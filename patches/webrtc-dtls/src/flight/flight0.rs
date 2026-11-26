use std::fmt;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use rand::Rng;

use super::flight2::*;
use super::*;
use crate::config::*;
use crate::conn::*;
use crate::curve::named_curve::NamedCurve;
use crate::error::Error;
use crate::extension::*;
use crate::handshake::*;
use crate::record_layer::record_layer_header::*;
use crate::*;

#[derive(Debug, PartialEq)]
pub(crate) struct Flight0;

impl fmt::Display for Flight0 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Flight 0")
    }
}

#[async_trait]
impl Flight for Flight0 {
    async fn parse(
        &self,
        _tx: &mut mpsc::Sender<mpsc::Sender<()>>,
        state: &mut State,
        cache: &HandshakeCache,
        cfg: &HandshakeConfig,
    ) -> Result<Box<dyn Flight + Send + Sync>, (Option<Alert>, Option<Error>)> {
        let (seq, msgs) = match cache
            .full_pull_map(
                0,
                &[HandshakeCachePullRule {
                    typ: HandshakeType::ClientHello,
                    epoch: cfg.initial_epoch,
                    is_client: true,
                    optional: false,
                }],
            )
            .await
        {
            Ok((seq, msgs)) => (seq, msgs),
            Err(_) => return Err((None, None)),
        };

        state.handshake_recv_sequence = seq;

        if let Some(message) = msgs.get(&HandshakeType::ClientHello) {
            // Validate type
            let client_hello = match message {
                HandshakeMessage::ClientHello(client_hello) => client_hello,
                _ => {
                    return Err((
                        Some(Alert {
                            alert_level: AlertLevel::Fatal,
                            alert_description: AlertDescription::InternalError,
                        }),
                        None,
                    ))
                }
            };

            if client_hello.version != PROTOCOL_VERSION1_2 {
                return Err((
                    Some(Alert {
                        alert_level: AlertLevel::Fatal,
                        alert_description: AlertDescription::ProtocolVersion,
                    }),
                    Some(Error::ErrUnsupportedProtocolVersion),
                ));
            }

            state.remote_random = client_hello.random.clone();

            if let Ok(id) =
                find_matching_cipher_suite(&client_hello.cipher_suites, &cfg.local_cipher_suites)
            {
                if let Ok(cipher_suite) = cipher_suite_for_id(id) {
                    log::debug!(
                        "[handshake:{}] use cipher suite: {}",
                        srv_cli_str(state.is_client),
                        cipher_suite.to_string()
                    );
                    let mut cs = state.cipher_suite.lock().await;
                    *cs = Some(cipher_suite);
                }
            } else {
                return Err((
                    Some(Alert {
                        alert_level: AlertLevel::Fatal,
                        alert_description: AlertDescription::InsufficientSecurity,
                    }),
                    Some(Error::ErrCipherSuiteNoIntersection),
                ));
            }

            for extension in &client_hello.extensions {
                match extension {
                    Extension::SupportedEllipticCurves(e) => {
                        let Some(curve) = e
                            .elliptic_curves
                            .iter()
                            .copied()
                            .find(|c| *c != NamedCurve::Unsupported)
                        else {
                            return Err((
                                Some(Alert {
                                    alert_level: AlertLevel::Fatal,
                                    alert_description: AlertDescription::InsufficientSecurity,
                                }),
                                Some(Error::ErrNoSupportedEllipticCurves),
                            ));
                        };
                        state.named_curve = curve;
                    }
                    Extension::UseSrtp(e) => {
                        if let Ok(profile) = find_matching_srtp_profile(
                            &e.protection_profiles,
                            &cfg.local_srtp_protection_profiles,
                        ) {
                            state.srtp_protection_profile = profile;
                        } else {
                            return Err((
                                Some(Alert {
                                    alert_level: AlertLevel::Fatal,
                                    alert_description: AlertDescription::InsufficientSecurity,
                                }),
                                Some(Error::ErrServerNoMatchingSrtpProfile),
                            ));
                        }
                    }
                    Extension::UseExtendedMasterSecret(_) => {
                        if cfg.extended_master_secret != ExtendedMasterSecretType::Disable {
                            state.extended_master_secret = true;
                        }
                    }
                    Extension::ServerName(e) => {
                        state.server_name = e.server_name.clone(); // remote server name
                    }
                    _ => {}
                }
            }

            if cfg.extended_master_secret == ExtendedMasterSecretType::Require
                && !state.extended_master_secret
            {
                return Err((
                    Some(Alert {
                        alert_level: AlertLevel::Fatal,
                        alert_description: AlertDescription::InsufficientSecurity,
                    }),
                    Some(Error::ErrServerRequiredButNoClientEms),
                ));
            }

            if state.local_keypair.is_none() {
                state.local_keypair = match state.named_curve.generate_keypair() {
                    Ok(local_keypar) => Some(local_keypar),
                    Err(err) => {
                        return Err((
                            Some(Alert {
                                alert_level: AlertLevel::Fatal,
                                alert_description: AlertDescription::IllegalParameter,
                            }),
                            Some(err),
                        ))
                    }
                };
            }

            Ok(Box::new(Flight2 {}))
        } else {
            Err((
                Some(Alert {
                    alert_level: AlertLevel::Fatal,
                    alert_description: AlertDescription::InternalError,
                }),
                None,
            ))
        }
    }

    async fn generate(
        &self,
        state: &mut State,
        _cache: &HandshakeCache,
        _cfg: &HandshakeConfig,
    ) -> Result<Vec<Packet>, (Option<Alert>, Option<Error>)> {
        // Initialize
        state.cookie = vec![0; COOKIE_LENGTH];
        rand::thread_rng().fill(state.cookie.as_mut_slice());

        //TODO: figure out difference between golang's atom store and rust atom store
        let zero_epoch = 0;
        state.local_epoch.store(zero_epoch, Ordering::SeqCst);
        state.remote_epoch.store(zero_epoch, Ordering::SeqCst);

        state.named_curve = DEFAULT_NAMED_CURVE;
        state.local_random.populate();

        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cipher_suite::CipherSuiteId;
    use crate::curve::named_curve::NamedCurve;
    use crate::extension::extension_use_srtp::SrtpProtectionProfile;
    use crate::fragment_buffer::FragmentBuffer;
    use crate::handshake::handshake_header::HandshakeHeader;
    use crate::record_layer::record_layer_header::RecordLayerHeader;
    use std::io::BufReader;

    #[tokio::test]
    async fn test_flight0_accepts_chrome_pq_grease_fragmented_client_hello() {
        // Simula um ClientHello moderno do Chrome (131-146+):
        // - session_id nao-vazio (32 bytes)
        // - GREASE + cipher suites TLS 1.3 + TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 (0xc02b)
        // - supported_groups com GREASE (0x2a2a), X25519MLKEM768 (0x11ec), X25519Kyber768 (0x6399), X25519 (0x001d), P256 (0x0017)
        // - extensoes DTLS 1.3 desconhecidas: supported_versions (0x002b) + key_share (0x0033) de 1216 bytes (>1400B total, fragmentado em 2 pacotes)
        let mut body = Vec::new();
        // client_version = DTLS 1.2 (0xfefd)
        body.extend_from_slice(&[0xfe, 0xfd]);
        // random (32 bytes)
        body.extend_from_slice(&[0x42; 32]);
        // session_id (32 bytes)
        body.push(32);
        body.extend_from_slice(&[0x11; 32]);
        // cookie (empty)
        body.push(0);
        // cipher_suites: GREASE (0x4a4a), TLS_AES_128_GCM_SHA256 (0x1301), 0xc02b
        let suites: [u16; 3] = [0x4a4a, 0x1301, 0xc02b];
        body.extend_from_slice(&((suites.len() * 2) as u16).to_be_bytes());
        for s in suites {
            body.extend_from_slice(&s.to_be_bytes());
        }
        // compression_methods: [0]
        body.extend_from_slice(&[1, 0]);

        let mut exts = Vec::new();
        // GREASE extension (0x6a6a, len 0)
        exts.extend_from_slice(&0x6a6au16.to_be_bytes());
        exts.extend_from_slice(&0u16.to_be_bytes());
        // supported_groups (0x000a): GREASE, X25519MLKEM768, X25519Kyber768, X25519, P256
        let groups: [u16; 5] = [0x2a2a, 0x11ec, 0x6399, 0x001d, 0x0017];
        exts.extend_from_slice(&0x000au16.to_be_bytes());
        exts.extend_from_slice(&((2 + groups.len() * 2) as u16).to_be_bytes());
        exts.extend_from_slice(&((groups.len() * 2) as u16).to_be_bytes());
        for g in groups {
            exts.extend_from_slice(&g.to_be_bytes());
        }
        // ec_point_formats (0x000b)
        exts.extend_from_slice(&[0x00, 0x0b, 0x00, 0x02, 0x01, 0x00]);
        // use_srtp (0x000e): SRTP_AEAD_AES_128_GCM (0x0007), SRTP_AES128_CM_HMAC_SHA1_80 (0x0001)
        exts.extend_from_slice(&[0x00, 0x0e, 0x00, 0x07, 0x00, 0x04, 0x00, 0x07, 0x00, 0x01, 0x00]);
        // extended_master_secret (0x0017)
        exts.extend_from_slice(&[0x00, 0x17, 0x00, 0x00]);
        // supported_versions (0x002b): DTLS 1.3 (0xfefc), DTLS 1.2 (0xfefd)
        exts.extend_from_slice(&[0x00, 0x2b, 0x00, 0x05, 0x04, 0xfe, 0xfc, 0xfe, 0xfd]);
        // key_share (0x0033): 1216 bytes simulating ML-KEM-768 client share
        exts.extend_from_slice(&0x0033u16.to_be_bytes());
        exts.extend_from_slice(&1216u16.to_be_bytes());
        exts.extend_from_slice(&vec![0xab; 1216]);

        body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        body.extend_from_slice(&exts);
        assert!(body.len() > 1300);

        let split_at = 900usize;
        let make_record = |seq: u64, offset: usize, chunk: &[u8]| -> Vec<u8> {
            let hh = HandshakeHeader {
                handshake_type: HandshakeType::ClientHello,
                length: body.len() as u32,
                message_sequence: 0,
                fragment_offset: offset as u32,
                fragment_length: chunk.len() as u32,
            };
            let mut hs_bytes = Vec::new();
            hh.marshal(&mut hs_bytes).unwrap();
            hs_bytes.extend_from_slice(chunk);

            let rlh = RecordLayerHeader {
                content_type: ContentType::Handshake,
                protocol_version: PROTOCOL_VERSION1_0,
                epoch: 0,
                sequence_number: seq,
                content_len: hs_bytes.len() as u16,
            };
            let mut rec = Vec::new();
            rlh.marshal(&mut rec).unwrap();
            rec.extend_from_slice(&hs_bytes);
            rec
        };

        let frag0 = make_record(0, 0, &body[..split_at]);
        let frag0_dup = make_record(1, 0, &body[..split_at]);
        let frag1 = make_record(2, split_at, &body[split_at..]);

        let mut fb = FragmentBuffer::new();
        assert!(fb.push(&frag0).unwrap());
        assert!(fb.push(&frag0_dup).unwrap());
        assert!(fb.pop().is_err());
        assert!(fb.push(&frag1).unwrap());
        let (reassembled, epoch) = fb.pop().expect("fragmented PQ ClientHello must reassemble");
        assert_eq!(epoch, 0);

        let mut reader = BufReader::new(reassembled.as_slice());
        let parsed = Handshake::unmarshal(&mut reader).expect("ClientHello must unmarshal");
        assert_eq!(parsed.handshake_header.handshake_type, HandshakeType::ClientHello);

        let mut cache = HandshakeCache::new();
        cache
            .push(reassembled, 0, 0, HandshakeType::ClientHello, true)
            .await;

        let mut state = State::default();
        let cfg = HandshakeConfig {
            local_cipher_suites: vec![CipherSuiteId::Tls_Ecdhe_Ecdsa_With_Aes_128_Gcm_Sha256],
            local_srtp_protection_profiles: vec![
                SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm,
                SrtpProtectionProfile::Srtp_Aes128_Cm_Hmac_Sha1_80,
            ],
            extended_master_secret: ExtendedMasterSecretType::Request,
            ..Default::default()
        };
        let (mut tx, _rx) = mpsc::channel(1);
        let next = Flight0 {}
            .parse(&mut tx, &mut state, &cache, &cfg)
            .await
            .expect("Flight0 must negotiate X25519 despite GREASE + X25519MLKEM768");
        assert_eq!(next.to_string(), "Flight 2");
        assert_eq!(state.named_curve, NamedCurve::X25519);
        assert!(state.local_keypair.is_some());
        assert!(state.extended_master_secret);
        assert_eq!(
            state.srtp_protection_profile,
            SrtpProtectionProfile::Srtp_Aead_Aes_128_Gcm
        );
    }
}
