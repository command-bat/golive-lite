# Arquitetura e Funcionamento Completo do GoLive (Sistema Web & API)

> **Documento de Engenharia Reversa e Referência Técnica de Alto Nível**  
> Destinado à equipe de desenvolvimento e agentes trabalhando no **GoLive Lite (`goliveRust`)**.  
> Baseado na análise aprofundada dos repositórios:
> - **Frontend**: `D:\projetos\goliveNT\group-sharescreen`
> - **Backend / API**: `D:\projetos\goliveNT\group-sharescreen-api`
> - **Data de Análise**: Novembro de 2026

---

## 1. Visão Geral do Ecossistema

O **GoLive** é uma plataforma distribuída de chamadas em tempo real focada em compartilhamento de tela com alta fidelidade (baixa latência, 60–120 FPS, 1080p a 4K), bate-papo por voz em grupo, chat de texto e ferramentas colaborativas em salas públicas e privadas.

### Topologia Geral do Sistema

```
+-------------------------------------------------------------------------------+
|                             CLIENTES GOLIVE                                   |
|                                                                               |
|  +------------------------+  +------------------------+  +-----------------+  |
|  | Web / Electron Client  |  | Mobile (Capacitor)     |  | GoLive Lite     |  |
|  | (group-sharescreen)    |  | (Android / iOS)        |  | (goliveRust)    |  |
|  +-----------+------------+  +-----------+------------+  +--------+--------+  |
+--------------|---------------------------|------------------------|-----------+
               |                           |                        |
               | [HTTP REST & WebSocket Sinalização]                |
               v                           v                        v
+-------------------------------------------------------------------------------+
|                       API & SINALIZAÇÃO (group-sharescreen-api)                |
|                                                                               |
|  Load Balancer (Nginx / Cloudflare)                                           |
|       │                                                                       |
|       ├──► Nó A (Primary Process) ──IPC──► [Worker 1] [Worker 2] [Worker N]   |
|       │         ▲ (Redis Bus)                                                 |
|       └──► Nó B (Primary Process) ──IPC──► [Worker 1] [Worker 2] [Worker N]   |
|                 ▼                                                             |
|       MongoDB (Contas, Logs, Moderação) | Redis (Cluster Bus & Rate Limits)   |
+-------------------------------------------------------------------------------+
                                       │
                      [Sinais WebRTC SDP & ICE Candidates]
                                       │
                                       ▼
+-------------------------------------------------------------------------------+
|                           MALHA DE MÍDIA (WebRTC Mesh)                        |
|                                                                               |
|   Cliente A ◄══════ [Áudio/Vídeo P2P Direto (UDP/SRTP)] ══════► Cliente B    |
|        ▲                                                             ▲        |
|        ╚═══════════════► Servidor TURN (Fallback NAT) ═══════════════╝        |
+-------------------------------------------------------------------------------+
```

---

## 2. O Backend (`group-sharescreen-api`)

O backend do GoLive é um servidor **Fastify** em TypeScript projetado para sustentar dezenas de milhares de conexões WebSocket persistentes e tráfego massivo com mínima latência.

### 2.1. Arquitetura de Concorrência e Cluster

Para superar a barreira da thread única do Node.js sem perder coerência de estado entre salas:

1. **Multi-Core Local (`node:cluster`)**:
   - `server/clusterPrimary.ts`: O processo primário não escuta conexões de rede externas. Ele supervisiona os workers filhos (1 por núcleo do processador via `availableParallelism()`).
   - **Barreira de Inicialização (`CLUSTER_BARRIER_MS`)**: Em restarts, todos os workers carregam o banco e stores em memória antes de abrir a porta TCP juntos, evitando que o primeiro worker a subir receba a avalanche de reconexões.
   - **Réplica Completa de Estado em Memória**: Cada worker mantém uma réplica do mapa global de `rooms` e `clients`. Se a conexão física de um usuário está no Worker 1 e outro usuário na mesma sala está no Worker 2, o Worker 1 possui um objeto `RemoteSocket` que repassa dados via IPC para o dono real do socket.

2. **Multi-Node Distribuído (`CLUSTER_MULTI_NODE=1`)**:
   - Várias máquinas físicas/containers comunicam-se via **Redis Pub/Sub** (`server/clusterBus.ts`).
   - Apenas os processos primários de cada máquina falam com o Redis, distribuindo em lote (batching) mensagens para seus workers locais via IPC, poupando a thread do Redis.
   - **Tolerância a Falhas**: Detecção de batimentos cardíacos (heartbeat a cada 5s). Máquina que cai é limpa automaticamente elegendo o nó sobrevivente com menor ID para emitir `peer-left`.

### 2.2. Protocolo de Sinalização WebSocket (`server/signaling.ts`)

A sinalização roda no endpoint `/ws`. As mensagens são objetos JSON tipados com o campo `type`:

#### A. Entrada e Registro na Sala (`join`)
O cliente envia:
```json
{
  "type": "join",
  "room": "nome-da-sala",
  "name": "NomeDoUsuario",
  "accountToken": "Bearer ... (opcional)",
  "confirmDevice": true,
  "streamerMode": false,
  "turnstileToken": "token-captcha (se exigido)"
}
```
**Regras de Negócio na API**:
- Verifica IP banido ou bloqueios por anti-spam (`moderationStore.ts`).
- Verifica nomes reservados por contas oficiais (`accountStore.ts`).
- Se a sala tiver senha ou for restrita a membros de um grupo, valida permissões.
- Checa se o usuário é assinante **GoLive Pro** (`entitlements.ts`), concedendo a flag `isPro`.

#### B. Resposta de Boas-Vindas (`welcome`)
A API responde para o novo participante:
```json
{
  "type": "welcome",
  "id": "uuid-do-cliente-no-servidor",
  "room": "nome-da-sala",
  "isPro": true,
  "peers": [
    {
      "id": "peer-uuid-1",
      "name": "Alice",
      "sharing": true,
      "screen": true,
      "camera": false,
      "device": 1,
      "isPro": false
    }
  ],
  "iceServers": [
    { "urls": "stun:stun.cloudflare.com:3478" }
  ]
}
```
Simultaneamente, a API dispara para todos os outros participantes da sala:
```json
{
  "type": "peer-joined",
  "peer": {
    "id": "uuid-do-novo-cliente",
    "name": "NomeDoUsuario",
    "sharing": false,
    "screen": false,
    "camera": false
  }
}
```

#### C. Encaminhamento de Sinais WebRTC (`signal`)
O backend atua estritamente como despachante de sinalização WebRTC entre os navegadores/clientes:
```json
{
  "type": "signal",
  "target": "id-do-peer-de-destino",
  "signal": {
    "type": "offer",
    "sdp": "v=0\r\no=... (SDP WebRTC completo)"
  }
}
```
O servidor apenas valida se a origem e o destino estão na mesma sala e repassa a mensagem para o socket correspondente:
```json
{
  "type": "signal",
  "sender": "id-do-peer-de-origem",
  "signal": { ... }
}
```
O mesmo mecanismo é usado para:
- `offer` (oferta SDP para abrir conexão ou novos transceivers).
- `answer` (resposta SDP contendo codecs aceitos).
- `candidate` (candidatos de rede ICE/STUN/TURN descobertos).

#### D. Mensagens de Chat (`chat`)
```json
{
  "type": "chat",
  "room": "nome-da-sala",
  "text": "Mensagem digitada"
}
```
A API sanitiza o texto, aplica filtros de palavras banidas, formata menções e replica para todos na sala com `id`, `senderId`, `senderName` e `timestamp`.

#### E. Presença de Transmissão (`sharing`)
Quando o cliente inicia ou encerra compartilhamento de tela ou câmera:
```json
{
  "type": "sharing",
  "screen": true,
  "camera": false,
  "files": []
}
```
A API atualiza o registro do peer e avisa a sala para que todos preparem a recepção de vídeo.

---

## 3. O Frontend (`group-sharescreen`)

O frontend é um aplicativo Next.js (React 19) com suporte a empacotamento desktop (Electron) e mobile (Capacitor).

### 3.1. Arquitetura WebRTC no Cliente (`useRoomMedia.ts`)

O GoLive opera primordialmente em arquitetura **WebRTC Mesh** (Full-Mesh ponto a ponto) para grupos pequenos/médios, complementado por transceivers direcionados:

1. **Gestão de PeerConnections (`RTCPeerConnection`)**:
   - Cada cliente mantém uma `RTCPeerConnection` dedicada para **cada outro membro** presente na sala.
   - Quando um novo membro entra (`peer-joined`), quem já estava na sala cria a oferta (`createOffer`), anexa as tracks de mídia locais e despacha via `signal`.
   - O membro que acabou de entrar recebe a oferta, aplica como `remoteDescription`, gera a `answer` e responde.

2. **Transceivers de Áudio e Vídeo**:
   - **Áudio (Voz)**: Adicionado via transceivers bidirecionais (`sendrecv`), permitindo que a voz de todos seja transmitida e recebida continuamente.
   - **Vídeo (Tela e Câmera)**: Adicionados via transceivers de vídeo (`sendonly` no transmissor, `recvonly` nos visualizadores).
   - **Múltiplas Telas (Multi-Screen Pro)**: Usuários Pro abrem slots extras de vídeo (`screen`, `screen2`, `screen3`), enviando fluxos independentes pela mesma conexão ou em streams agrupados.

3. **Cancelamento de Ruído e Áudio (`rnnoise.ts` e `speakingDetector.ts`)**:
   - O áudio bruto do microfone (`getUserMedia`) passa por um nó de processamento de áudio via WebAssembly (RNNoise) para remoção de ruídos de fundo.
   - Um nó `AnalyserNode` calcula a energia RMS (Root Mean Square) das frequências vocais. Quando a energia ultrapassa o limiar, o cliente marca localmente que o usuário está falando e aciona a borda verde visual no tile.

4. **Controle de Qualidade Adaptativo (`videoQuality.ts` e `peerQualityController.ts`)**:
   - O GoLive possui uma escada de qualidade rigorosamente calibrada:
     - Free: Limitado a 720p 30 FPS.
     - Pro: Acesso a 1080p, 1440p (2K), 2160p (4K), 60 FPS e 120 FPS.
   - **Negociação Dinâmica**: O transmissor ajusta o bitrate de saída (`maxBitrate` no `RTCRtpSender.setParameters`) com base nas mensagens de feedback RTCP dos visualizadores (perda de pacotes e largura de banda estimada).

---

## 4. O Sistema GoLive Pro: Funcional vs. Cosmético

Ao estudar o código de `entitlements.ts`, `accountStore.ts` e `videoQuality.ts`, verifica-se uma separação clara entre as vantagens Pro:

### A. Recursos Funcionais (Críticos para Transmissão)
- **Qualidade de Transmissão Elevada**: Acesso a resoluções até 4K (3840x2160) e taxas de quadros de 60 e 120 FPS.
- **Múltiplas Telas Simultâneas**: Capacidade de transmitir mais de um monitor ao mesmo tempo (slot `screen` e `screen2`).

### B. Recursos Cosméticos (Não Funcionais)
- Selos e badges Pro coloridos ao lado do nome do usuário.
- Temas de cores customizados para a interface da sala.
- Molduras decorativas ao redor do avatar.
- Avatares animados / efeitos sonoros de entrada na sala.

> **Decisão de Design para o GoLive Lite (`goliveRust`)**:  
> Conforme estipulado no manifesto `AGENTS.md`, **apenas os recursos funcionais Pro são mantidos**. Toda a camada cosmética é eliminada do cliente Rust para preservar o consumo mínimo de RAM e CPU.

---

## 5. Como o `goliveRust` se Conecta e Opera no Ecossistema

Para que o **GoLive Lite** funcione perfeitamente com os servidores e clientes oficiais existentes sem qualquer alteração no backend:

### Tabela de Mapeamento: GoLive Web vs. GoLive Rust

| Funcionalidade | GoLive Web / Electron (`group-sharescreen`) | GoLive Lite (`goliveRust`) |
|---|---|---|
| **Runtime & GUI** | Chromium / React / Tailwind (RAM: 300MB–800MB) | Rust / `egui` nativo imediato (RAM: 20MB–45MB) |
| **Sinalização** | WebSocket JS (`signalingClient.ts`) | `tokio-tungstenite` em Rust (`src/net/signaling.rs`) |
| **WebRTC Core** | LibWebRTC nativo do navegador Chrome | `webrtc-rs` (Rust puro e assíncrono) |
| **Codec de Áudio** | WebRTC Opus nativo do navegador | `cpal` (captura) + `opus` crate em modo VoIP 24kbps |
| **Captura de Tela** | `navigator.mediaDevices.getDisplayMedia` | `xcap` / Windows Graphics Capture nativo (Zero-Copy) |
| **Captura de Câmera** | `navigator.mediaDevices.getUserMedia` | `nokhwa` (captura nativa direta) |
| **Decodificação de Vídeo** | Decodificador interno do Chromium | Decodificador de hardware via OS / H.264 leve em textura egui |
| **Ferramentas de Sala** | Lousa, anotações, desenhos, YouTube sincronizado | **Desativadas**. Streams chegam e renderizam puramente como vídeo |
| **Cosméticos** | Badges, temas, bordas de avatar animadas | **Desativados**. Apenas dark theme limpo nativo |

### Ciclo Completo de Execução no `goliveRust`

1. **Inicialização**:
   - Carrega `src/config.rs` (último nome usado, token de conta Pro opcional, volumes salvos).
   - Abre janela nativa `eframe` configurada com taxa de quadros sob demanda (`request_repaint`).
2. **Entrada na Sala**:
   - Conecta ao WebSocket `wss://apigolive.nemtudo.me/ws`.
   - Envia o JSON `type: "join"` idêntico ao do cliente web.
   - Recebe `welcome`, extrai lista de `peers` e a flag `isPro`.
3. **Estabelecimento de Mídia (WebRTC)**:
   - Para cada peer na sala, inicia uma `RTCPeerConnection` via `webrtc-rs`.
   - Adiciona track de áudio local (capturada via `cpal` e comprimida com `opus`).
   - Se o usuário clicar em "Compartilhar Tela", captura o framebuffer nativo via `xcap`, envia para o codificador de vídeo (na resolução/FPS permitida pela conta Free ou Pro) e dispara `type: "sharing"`.
4. **Recepção de Streams Remotos**:
   - Áudio de outros participantes é decodificado via `opus`, passa pelo mixador de volume individual (`audio_mixer.rs`) e vai para a saída de som.
   - Vídeos de outros participantes (telas e webcams) são decodificados quadro a quadro, atualizando a textura GPU de cada tile no grid do `egui`.
5. **Chat e Presença**:
   - Mensagens de chat fluem bidirecionalmente pelo mesmo WebSocket de sinalização, atualizando a lista de mensagens na UI.

---

## 6. Conclusão

O GoLive possui uma arquitetura backend de sinalização simples, elegante e altamente escalável baseada em WebSockets JSON e malha WebRTC. 

O cliente **GoLive Lite (`goliveRust`)** conecta-se exatamente como qualquer outro cliente regular ao backend oficial, usufruindo da mesma infraestrutura de salas, sinalização e conexões de mídia, porém eliminando 95% do overhead de CPU e memória ao descartar a engine web e os recursos cosméticos desnecessários.
