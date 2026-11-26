# AGENTS.md — GoLive Lite (`goliveRust`)

> **Documento de Especificação Arquitetural e Diretrizes para Agentes de IA e Desenvolvedores**  
> Projeto: **GoLive Lite (Cliente Nativo Oficial em Rust)**  
> Repositório: `d:\projetos\goliveNT\goliveRust`  
> Última Atualização: 2026-11-22

---

## 1. Visão Geral e Filosofia do Projeto

O **GoLive Lite (`goliveRust`)** é um cliente oficial nativo, minimalista e hiper-otimizado do ecossistema GoLive, desenvolvido integralmente em **Rust**.

### O Problema que Resolve
O cliente principal baseado em Web/Electron consome centenas de megabytes de memória RAM (250MB–800MB+), gera uso considerável de CPU em máquinas mais fracas e possui instaladores pesados devido à engine Chromium/Node.js.

### O Objetivo Supremo: Eficiência Absoluta
O GoLive Lite tem uma missão inegociável: **ser o cliente de voz, vídeo e tela mais leve possível existente na plataforma desktop**, operando com consumo insignificante de:
1. **Armazenamento em Disco**: Binário compilado único, estático, sem runtimes externos (meta: **5 MB a 15 MB**).
2. **Memória RAM**: Consumo mínimo mesmo no pico de uso com múltiplos streams ativos (meta: **20 MB a 45 MB** em repouso/voz; máximo de **50 MB a 80 MB** recebendo vídeos).
3. **Processamento (CPU)**: Uso residual de CPU (**< 1% a 3%** em repouso/voz; **< 5% a 8%** decodificando vídeo em máquinas modestas).
4. **Rede e Largura de Banda**: Otimização estrita de tráfego (Opus low-bitrate dinâmico para voz, silenciamento quando mutado, recepção adaptativa de vídeo).

> ⚠️ **REGRA DE OURO PARA AGENTES**: Qualquer adição de biblioteca, código ou dependência que introduza overhead de memória, alocações contínuas no loop principal, ou dependências pesadas de C++/Chromium deve ser sumariamente rejeitada.

---

## 2. Escopo Cirúrgico: O que entra vs. O que é cortado

Para garantir essa leveza extrema, o GoLive Lite é intencionalmente **limitado** em escopo em relação ao cliente web completo:

### ✅ Requisitos Obrigatórios (Core Features)
1. **Entrar em salas a partir de um código/handle**:
   - Suporte a digitar/colar o código da sala (ex: `amigos`, `priv-sala123`) ou link completo (`golive.nemtudo.me/watch/codigo` ou `g.nemtudo.me/codigo`).
   - Conexão nativa e direta via WebSocket ao servidor de sinalização do GoLive.
2. **Chat de Voz Bidirecional**:
   - Captura de áudio nativa do microfone com codec Opus em baixo bitrate (16–32 kbps para voz).
   - Detecção de atividade de voz (VAD) ou Push-to-Talk (não envia pacotes no silêncio).
   - Reprodução e mixagem de áudio dos participantes.
3. **Chat de Texto em Tempo Real**:
   - Envio e recebimento instantâneo de mensagens de texto na sala.
   - Visualização limpa com timestamps e nomes dos usuários.
4. **Compartilhamento e Recepção de Mídias**:
   - **Envio (Transmissão Local)**: Apenas compartilhamento de **Tela** (captura nativa de alta performance com baixo uso de CPU) e **Câmera**.
   - **Recepção (Visualização Remota)**: O cliente **consegue ver todas as mídias da chamada** que os outros participantes estiverem transmitindo (telas, webcams, etc.).
   - **Sem ferramentas interativas**: Ferramentas interativas pesadas (desenhos, anotações sobre a tela, lousa virtual, jogos, player de música sincronizada local) **NÃO funcionam e NÃO são implementadas**. Os streams chegam e são renderizados puramente como vídeo.
5. **Funções Completas de Mute e Áudio**:
   - **Mute do próprio microfone** (Mute / Unmute / Push-to-Talk).
   - **Ensurdecer (Deafen)**: Cortar instantaneamente a saída de áudio de todos na sala.
   - **Ajuste de volume individual**: Slider de ganho (0% a 200%) e mute individual para cada participante da sala.
6. **Layout e Identidade Visual Fiéis ao GoLive**:
   - Visual moderno, minimalista e idêntico ao GoLive web: Dark theme característico (`#0f0f11`, `#18181b`, `#27272a`, acentos azuis `#3b82f6`), grid inteligente de tiles com cantos arredondados, barra de controles inferior com botões circulares, drawer lateral de chat retrátil e indicação luminosa de fala (halo/borda verde).

---

### ⭐ Funcionalidades PRO Suportadas

O GoLive Lite inclui suporte a recursos GoLive Pro focados exclusivamente em **capacidade técnica de transmissão**:

1. **Alteração da Qualidade da Transmissão (Resolução, FPS e Bitrate)**:
   - **Usuário Gratuito (Free)**: Transmissão padrão fixa em **720p a 30 FPS** (com bitrate conservador para poupar banda e CPU).
   - **Usuário Pro**: Desbloqueio de seletores de qualidade técnica no menu de compartilhamento de tela:
     - **Resoluções**: 720p, 1080p (Full HD), 1440p (2K) e 2160p (4K).
     - **Taxas de Quadros (FPS)**: 30 FPS, 60 FPS e 120 FPS.
     - **Bitrate Adaptativo**: O codificador de vídeo ajusta o teto de bitrate (`encoderCeilingKbps`) de acordo com o tier Pro selecionado, garantindo nitidez para jogos ou textos de trabalho.
2. **Quantidade de Telas Simultâneas (Multi-Screen Share)**:
   - **Usuário Gratuito (Free)**: Limite de **1 tela** compartilhada por vez.
   - **Usuário Pro**: Capacidade de transmitir **múltiplas telas/monitores em simultâneo** (ex: Monitor 1 e Monitor 2 ao mesmo tempo, gerando tiles separados na chamada para os espectadores).

### 🚫 Exclusão Estrita de Cosméticos Pro
- **ZERO Cosméticos Pro**: Funções cosméticas do GoLive Pro (badges Pro decorativos ao lado do nome, molduras personalizadas de avatar, avatares animados, temas visuais customizados adicionais, efeitos de entrada na sala) **NÃO DEVEM SER IMPLEMENTADAS**.
- O cliente prioriza 100% a utilidade funcional e o consumo mínimo de RAM/CPU. Elementos cosméticos adicionam parsing desnecessário, dependências de imagem/animação e uso inútil de memória.

---

## 3. Stack Tecnológica Recomendada

Para atingir esses limites sem precedentes, **NÃO** usamos Electron, Tauri com WebView2 (que injeta o Chromium de 150MB+ de RAM), nem engines web.

| Camada | Tecnologia Rust | Motivo da Escolha |
|---|---|---|
| **Linguagem** | Rust (Edition 2021+) | Zero garbage collection, controle absoluto de memória e concorrência nativa segura. |
| **GUI Framework** | **egui** (`eframe` com backend `glow` / `wgpu`) | Immediate mode GUI ultra-leve. Consumo de RAM: ~15MB a 30MB. Zero WebViews, renderização sob demanda (`request_repaint` apenas quando há alterações ou novo frame de vídeo). |
| **Alternativa GUI** | **Slint** | GUI declarativa compilada nativa com pegada de memória minúscula e renderização por software ou hardware. |
| **Async Runtime** | **tokio** | Runtime assíncrono padrão da indústria com thread pool mínima configurada para poupar threads. |
| **WebSocket / Sinalização** | **tokio-tungstenite** / **fastwebsockets** | WebSocket nativo, veloz, sem dependências volumosas. |
| **Protocolo WebRTC** | **webrtc-rs** | Implementação WebRTC nativa e pura em Rust (sem o monólito de 50MB+ da libwebrtc C++). |
| **Áudio I/O** | **cpal** | I/O de áudio multiplataforma de baixo nível (WASAPI no Windows, ALSA/Pulse no Linux, CoreAudio no macOS). |
| **Codec de Áudio** | **opus** (`audiopus` ou bindings Rust para libopus) | Padrão da internet para voz em tempo real de altíssima eficiência e compressão extrema. |
| **Captura de Tela** | **xcap** ou Windows Graphics Capture nativo | Captura direta do framebuffer com aceleração de hardware, seleção de múltiplos monitores e cópia zero (Zero-Copy). |
| **Captura de Câmera** | **nokhwa** | Captura direta e ultra-leve de dispositivos de vídeo UVC. |
| **Codificação/Decodificação de Vídeo** | Decodificadores H.264 / VP8 nativos do OS ou minimalistas (ex: Media Foundation / OpenH264) | Aceleração por hardware do sistema operacional para manter CPU < 3% em Free e escalável em Pro. |

---

## 4. Arquitetura do Sistema e Estrutura de Diretórios

```
goliveRust/
├── Cargo.toml                  # Configurações do compilador, dependências e flags de otimização
├── AGENTS.md                   # Este manifesto de arquitetura e instruções
├── assets/                     # Ícones, fontes leves e recursos visuais mínimos
│   └── icon.ico
└── src/
    ├── main.rs                 # Ponto de entrada: inicialização do Tokio e launch do eframe/egui
    ├── app.rs                  # Estado principal da aplicação e máquina de estados da UI
    ├── config.rs               # Persistência local mínima (nome, token de autenticação/Pro, volumes)
    ├── net/
    │   ├── mod.rs
    │   ├── signaling.rs        # Cliente WebSocket de sinalização GoLive (protocolo JSON)
    │   ├── webrtc.rs           # Gerenciamento de PeerConnections, SDP e ICE candidates
    │   ├── auth.rs             # Validação leve do token de conta / status Pro
    │   └── protocol.rs         # Estruturas Serde dos eventos do GoLive (join, welcome, chat, sharing)
    ├── media/
    │   ├── mod.rs
    │   ├── audio.rs            # Captura de microfone (CPAL), encoder Opus, VAD e jitter buffer
    │   ├── audio_mixer.rs      # Mixagem de canais de voz, ganho por usuário e master deafen
    │   ├── screen_capture.rs   # Captura ultra-leve de tela / múltiplos monitores (Multi-Screen Pro)
    │   ├── camera_capture.rs   # Captura de webcam
    │   ├── video_encoder.rs    # Codificador de vídeo com tiers de qualidade (720p até 4K / 30-120fps)
    │   └── video_decoder.rs    # Decodificação de streams de vídeo recebidos em texturas egui
    └── ui/
        ├── mod.rs
        ├── theme.rs            # Cores, fontes, espaçamentos idênticos ao design do GoLive
        ├── components/
        │   ├── mod.rs
        │   ├── button.rs       # Botões estilizados com visual pill e circular do GoLive
        │   ├── tile.rs         # Tile de vídeo/participante (borda de voz, nome, ícones)
        │   └── slider.rs       # Slider customizado para ajuste de volume
        └── views/
            ├── mod.rs
            ├── home_view.rs    # Tela inicial: input de código da sala, nome, token opcional e entrar
            ├── room_view.rs    # Tela da sala: grid de vídeos, barra de controle e chat
            ├── chat_drawer.rs  # Painel lateral retrátil de chat de texto
            ├── audio_panel.rs  # Popover de ajuste de volume dos participantes e mic
            └── share_modal.rs  # Modal de seleção de tela(s), qualidade (Resolução/FPS - Pro)
```

---

## 5. Protocolo de Rede e Sinalização do GoLive

O cliente se comunica diretamente com o backend de sinalização do GoLive (`wss://apigolive.nemtudo.me/ws` ou `ws://localhost:4000/ws`).

### Fluxo de Comunicação
1. **Conexão WebSocket**:
   - Conectar ao endpoint WebSocket.
2. **Entrada na Sala (`join`)**:
   - Po
   - de enviar `accountToken` se o usuário tiver configurado sua conta para validar status Pro:
   ```json
   {
     "type": "join",
     "room": "nome-da-sala",
     "name": "NomeDoUsuario",
     "accountToken": "token-opcional-pro",
     "confirmDevice": true,
     "streamerMode": false
   }
   ```
3. **Recepção de Boas-Vindas (`welcome`)**:
   - O servidor responde com o ID atribuído ao cliente, status do usuário e a lista de peers já conectados:
   ```json
   {
     "type": "welcome",
     "id": "client-uuid-1234",
     "isPro": true,
     "peers": [
       { "id": "peer-1", "name": "Alice", "sharing": false, "screen": false, "camera": false }
     ]
   }
   ```
4. **Troca de WebRTC (`signal`)**:
   - Encaminhamento de ofertas, respostas e candidatos ICE:
   ```json
   {
     "type": "signal",
     "target": "peer-id",
     "signal": {
       "type": "offer",
       "sdp": "v=0..."
     }
   }
   ```
5. **Chat de Texto em Tempo Real (`chat`)**:
   - Envio:
     ```json
     {
       "type": "chat",
       "room": "nome-da-sala",
       "text": "Olá a todos!"
     }
     ```
   - Recebimento:
     ```json
     {
       "type": "chat",
       "id": "msg-uuid",
       "senderId": "peer-1",
       "senderName": "Alice",
       "text": "Olá a todos!",
       "timestamp": 1732312500000
     }
     ```
6. **Anúncio de Transmissão de Mídia (`sharing`)**:
   - Informa aos outros participantes as mídias ativas. Em modo Pro com múltiplas telas, suporta múltiplos canais de transmissão de tela:
   ```json
   {
     "type": "sharing",
     "screen": true,
     "camera": false,
     "screenCount": 2
   }
   ```

---

## 6. Especificação Visual e de Layout (Fiel ao GoLive)

O design transmite a identidade visual clássica do GoLive: elegante, minimalista, dark theme e foco no conteúdo.

### Paleta de Cores
- **Fundo Principal (Background)**: `#09090b` / `#0f0f11` (Preto profundo / Zinco muito escuro).
- **Cards e Painéis (Surface)**: `#18181b` com bordas sutis em `#27272a`.
- **Destaque Primário (Accent)**: `#3b82f6` (Azul GoLive) e hover em `#2563eb`.
- **Indicador de Fala Ativa (Speaking)**: `#22c55e` (Verde esmeralda brilhante) contornando o tile do participante.
- **Estado Mutado / Perigo**: `#ef4444` (Vermelho).
- **Texto Principal**: `#f4f4f5` (Branco suave); **Texto Secundário**: `#a1a1aa` (Cinza claro).

### Diagrama do Layout da Sala de Chamada
```
+-----------------------------------------------------------------------------------+
|  [Logo GoLive]   Sala: #estudos-rust    [Copiar Código]              [Sair da Sala]  |
+-----------------------------------------------------------------+-----------------+
|                                                                 |  CHAT DA SALA   |
|   +--------------------------+   +--------------------------+   |                 |
|   |                          |   |                          |   | [19:04] Alice:  |
|   |                          |   |                          |   | Fala pessoal!   |
|   |    [ Stream de Tela ]    |   |     [ Webcam do Bob ]    |   |                 |
|   |      (1080p 60fps)       |   |                          |   | [19:05] Você:   |
|   |  (Alice - Compartilhando)|   |  (Bob) [🎤 Mutado]       |   | Ouvindo bem...  |
|   +--------------------------+   +--------------------------+   |                 |
|                                                                 |                 |
|   +--------------------------+   +--------------------------+   |                 |
|   |       [ Avatar ]         |   |       [ Avatar ]         |   |                 |
|   |         (Você)           |   |         (Carlos)         |   |                 |
|   |   [Borda Verde: Falando] |   |       [Vol: 80%]         |   |                 |
|   +--------------------------+   +--------------------------+   |                 |
|                                                                 +-----------------+
|                                                                 | Digite aqui...  |
+-----------------------------------------------------------------+-----------------+
|               [ 🎤 Mic ]  [ 🎧 Deafen ]  [ 🖥️ Tela ]  [ 📷 Cam ]  [ 💬 Chat ]      |
+-----------------------------------------------------------------------------------+
```

### Controles Inferiores (Dock Flutuante)
- **Botão Microfone**: Alterna mudo/desmudo local. Fica vermelho com ícone cortado quando mutado. Clique com botão direito ou clique longo abre menu de sensibilidade / seleção de microfone.
- **Botão Deafen (Ensurdecer)**: Desliga saída de áudio de todos os participantes. Ícone de fone de ouvido cortado em vermelho quando ativo.
- **Botão Tela**: 
  - Abre o modal/popover de compartilhamento nativo.
  - Permite escolher qual monitor/janela compartilhar.
  - **Se Pro**: Permite selecionar mais de uma tela simultânea (checkbox em cada monitor detectado) e configurar a **Qualidade da Transmissão** (Resolução: 720p, 1080p, 1440p, 4K | FPS: 30, 60, 120).
  - **Se Free**: Limite de 1 tela fixa em 720p 30 FPS.
- **Botão Câmera**: Inicia/interrompe transmissão da webcam.
- **Botão Chat**: Abre/fecha o painel lateral de mensagens.
- **Menu de Participantes & Volumes**: Permite ajustar o slider de volume (0% a 200%) individual de qualquer usuário presente na sala.

---

## 7. Diretrizes Críticas de Otimização e Desempenho

Qualquer código escrito no projeto deve seguir estas regras sem exceção:

### 1. Zero Renderização Contínua Desnecessária
- No `egui`, utilize a renderização reativa sob demanda (`request_repaint_after` / `request_repaint`).
- A interface gráfica **NÃO DEVE** rodar a 60 FPS contínuos em repouso.
- Se nada mudou na tela e não há novos frames de vídeo, a taxa de atualização cai para 0 FPS (dorme aguardando evento de entrada ou sinal de rede).
- Ao receber vídeo, apenas a textura do tile atualizado é repintada na taxa do stream recebido (ex: 30 FPS).

### 2. Gerenciamento Estrito de Memória (Zero-Alloc no Hot Path)
- Buffers de áudio (CPAL e Opus) devem ser pré-alocados em estruturas circulares ou reutilizados (`Vec::clear()` em vez de recriar vetores a cada pacote).
- Frames de vídeo capturados ou recebidos devem reutilizar o mesmo buffer de textura GPU, atualizando os pixels via `egui::ColorImage` ou buffers de textura nativos sem realocação.
 
### 3. VAD (Voice Activity Detection) e Economia de Rede
- Se o microfone estiver mutado: a captura de áudio é suspensa ou descartada imediatamente antes de atingir o encoder Opus.
- Se o microfone estiver aberto mas em silêncio (abaixo do limiar de ruído): nenhum pacote UDP/RTP é transmitido.
- O codec Opus deve ser configurado no modo `Application::Voip` com bitrate contido (ex: 24 kbps), economizando CPU e banda.

### 4. Configuração Extrema do Compilador (`Cargo.toml`)
Para garantir o binário de 5MB a 15MB:
```toml
[profile.release]
opt-level = "z"          # Otimização máxima para tamanho do binário
lto = true               # Link-Time Optimization em todo o código
codegen-units = 1        # Máxima otimização inter-módulos
panic = "abort"          # Remove tabelas de unwinding de exceção
strip = true             # Remove símbolos e nomes de debug
```

---

## 8. Roteiro de Implementação Passo a Passo (Roadmap)

Os agentes de IA devem executar e guiar a construção do projeto nas seguintes fases estritas:

### Fase 1: Fundação do Projeto e Shell da UI
- [ ] Inicializar o workspace Cargo com flags de otimização no `Cargo.toml`.
- [ ] Implementar a estrutura básica em `egui` / `eframe`.
- [ ] Criar a tela inicial (`HomeView`): campo para código da sala (`handle`), campo para nome de usuário, campo opcional de token Pro e botão "Entrar".
- [ ] Criar a tela da sala (`RoomView`): layout base com barra superior, grid de tiles vazios, barra de ferramentas inferior e drawer lateral de chat.
- [ ] Aplicar o tema escuro idêntico ao GoLive (sem penduricalhos cosméticos).

### Fase 2: Conexão e Sinalização GoLive
- [ ] Implementar cliente WebSocket assíncrono em `src/net/signaling.rs` usando `tokio` e `tokio-tungstenite`.
- [ ] Modelar mensagens de protocolo (`join`, `welcome`, `peer-joined`, `peer-left`, `chat`, `signal`, `sharing`).
- [ ] Tratar a flag `isPro` retornada no `welcome` ou validada via token.
- [ ] Integrar sinalização com a UI: ao clicar em "Entrar", conectar à sala do GoLive e listar participantes em tempo real nos tiles.
- [ ] Integrar o chat de texto: envio e recebimento de mensagens na sala.

### Fase 3: Pipeline de Áudio Nativo (Voz, Mute, Deafen e Volume por Usuário)
- [ ] Implementar captura e reprodução de áudio de baixo nível com `cpal`.
- [ ] Integrar codificador e decodificador `opus`.
- [ ] Adicionar VAD (detecção de voz) simples para halo verde no tile e economia de banda.
- [ ] Implementar controle de mute próprio, atalho de Push-to-Talk e botão Deafen (ensurdecer geral).
- [ ] Implementar mixador de áudio (`audio_mixer.rs`) com controle de ganho individual por peer (0% a 200%).

### Fase 4: Pipeline de Vídeo, Transmissão e Recursos Pro de Mídia
- [ ] Integrar `webrtc-rs` para estabelecer conexões peer-to-peer / SFU com os participantes da sala.
- [ ] Implementar decodificação e renderização de vídeo recebido (telas e webcams de outros participantes) diretamente nas texturas dos tiles.
- [ ] Implementar captura de tela nativa (`screen_capture.rs`):
  - Modo Free: Seleção de 1 monitor, saída em 720p 30 FPS.
  - Modo Pro: Seleção de **múltiplos monitores simultâneos** (Multi-Screen Share) e seletor de **Qualidade de Transmissão** (1080p, 1440p, 4K | 30, 60, 120 FPS).
- [ ] Implementar transmissão local de **Câmera** (captura webcam via `nokhwa`).

### Fase 5: Profiling, Validação de Métricas e Empacotamento
- [ ] Medir consumo de memória RAM em repouso e em chamada ativa (garantir meta < 50MB no Free e < 80MB no Pro com múltiplos streams).
- [ ] Medir consumo de CPU durante chamada e transmissão (garantir meta < 5%).
- [ ] Validar tamanho final do executável compilado (5 MB a 15 MB).
- [ ] Adicionar suporte a builds em Windows (`.exe` standalone portátil).

---

## 9. Instruções para Agentes Autônomos

Quando for solicitado a desenvolver ou modificar código no `goliveRust`:
1. **Consulte este AGENTS.md antes de qualquer ação**: Ele é a verdade única sobre o que o projeto deve ser.
2. **Priorize Sempre Métricas de Consumo**: Se houver duas abordagens para resolver um problema, escolha invariavelmente a que usa menos RAM, menos ciclos de CPU e menos bytes no binário.
3. **Não Adicione Cosméticos Pro nem Features Extras**: Recursos cosméticos (badges, molduras, temas extras, animações) estão terminantemente proibidos. Os recursos Pro permitidos são exclusivamente: **qualidade da transmissão (resolução/fps/bitrate)** e **quantidade de telas transmitidas em paralelo**.
4. **Mantenha o Código Idiomático e Seguro em Rust**: Evite `unsafe` des
5. necessário, trate erros de rede e WebRTC de forma resiliente para nunca derrubar o processo (`panic-free`).
6. **OBRIGATÓRIO — Cópia 100% fiel do site GoLive**: toda interface do Go Live - Lite (principalmente dentro das salas) deve ser idêntica ao site/Electron oficial (`D:\projetos\goliveNT\group-sharescreen`): layout, UI, UX, design, cores, tamanhos, espaçamentos, ícones, textos e comportamento. Antes de mexer numa tela, leia o componente correspondente do site (sala: `app/(shell)/watch/[handle]/WatchRoom.tsx`, `components/ParticipantRow.tsx`, `components/ChatPanel.tsx`, `components/VideoTile.tsx` etc.) e traduza as classes Tailwind literalmente (px, `rounded-*`, `gap-*`, `p-*`). Layout e cores = o da sala nova (experiment `room-redesign`, camada `[data-room-redesign]` em `app/globals.css`): página `#0d1117`, zinc-950 `#161b22`, zinc-900 `#1c222b`, zinc-800 `#2a313c`, zinc-700 `#3a424e`; texto `#e9e9ec`; fonte Plus Jakarta Sans; cantos 14/20/28 px. Textos em pt-BR iguais aos de `locales/pt.json`. Não invente UI que o site não tem. Vale para todo pedido futuro, mesmo que o pedido não mencione isso.
7. **Nome e logo**: sempre "Go Live - Lite"; o logo é o do GoLive com um raio no meio.
8. **Sempre recompile e gere o instalador** (`build-installer.ps1` → `dist/Go-Live-Lite-Setup-<versão>.exe`) ao terminar uma mudança.
9. **Métricas (admin do Go Live → Go Live - Lite → Métricas)**: não quebre a cadeia que alimenta os números. O setup gera o `install_id` (32 hex) no `config.json`; o app manda esse id no `register` com `liteVersion` (= `CARGO_PKG_VERSION`) e `liteMods` (`modifications.json`); a cada 10 min manda `lite-stats` (RAM/CPU/mídias, `win_tray::lite_usage_sample`, contadores `MEDIA_IN`/`MEDIA_OUT`); o setup manda `/lite/setup-event` e o desinstalador `/lite/uninstall`. `GOLIVE_LITE_NO_METRICS=1` desliga os eventos do setup em testes. Detalhes: `group-sharescreen-api/README.md`, seção "Métricas do Lite".
10. **Modificações novas (padrão obrigatório, para não gastar nada de quem não instalou)**: cada uma é uma feature do Cargo (`mod_<id>`, no `default` do `Cargo.toml`), um id em `config.rs` (`MOD_<ID>`, igual ao id do catálogo da API) e uma linha em `GoLiveLiteApp::has_modification`. Todo o código fica atrás de `#[cfg(feature = "mod_<id>")]` (módulos próprios em `src/net/` e `src/ui/`), e o estado é um `Option<Box<...>>` no app que só é criado quando `has_modification` é verdadeiro — nada de threads, timers ou polling: rede só por evento (login, mensagem do WebSocket, clique). Exemplo completo: "Amigos" (`net/social.rs`, `ui/friends_view.rs`, campo `app.amigos`). Ao criar, também: entrada `active` no seed de `group-sharescreen-api/server/liteStore.ts`, o id em `BUILTIN_MODS` do instalador, e ativar no admin (aba Go Live - Lite > Modificações) para o catálogo de produção.
