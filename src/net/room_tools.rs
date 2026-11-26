//! Motor colaborativo leve das Ferramentas da Sala (espela `lib/roomTools.ts` + `lib/textOt.ts`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

// ---------------------------------------------------------------------------
// OT de texto compativel com UTF-16 do GoLive Web (`lib/textOt.ts` / `textSync.ts`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum TextPrim {
    #[serde(rename = "i")]
    Insert { p: usize, s: String },
    #[serde(rename = "d")]
    Delete { p: usize, n: usize },
}

pub fn apply_text_op(text: &str, op: &[TextPrim]) -> String {
    let mut u16: Vec<u16> = text.encode_utf16().collect();
    for prim in op {
        let len = u16.len();
        match prim {
            TextPrim::Insert { p, s } => {
                let idx = (*p).min(len);
                let ins: Vec<u16> = s.encode_utf16().collect();
                u16.splice(idx..idx, ins);
            }
            TextPrim::Delete { p, n } => {
                let idx = (*p).min(len);
                let end = (idx + *n).min(len);
                u16.drain(idx..end);
            }
        }
    }
    String::from_utf16_lossy(&u16)
}

pub fn diff_text_utf16(before: &str, after: &str) -> Vec<TextPrim> {
    if before == after {
        return Vec::new();
    }
    let a: Vec<u16> = before.encode_utf16().collect();
    let b: Vec<u16> = after.encode_utf16().collect();
    let mut start = 0;
    let max_pref = a.len().min(b.len());
    while start < max_pref && a[start] == b[start] {
        start += 1;
    }
    let mut end_a = a.len();
    let mut end_b = b.len();
    while end_a > start && end_b > start && a[end_a - 1] == b[end_b - 1] {
        end_a -= 1;
        end_b -= 1;
    }
    let mut op = Vec::new();
    if end_a > start {
        op.push(TextPrim::Delete {
            p: start,
            n: end_a - start,
        });
    }
    if end_b > start {
        op.push(TextPrim::Insert {
            p: start,
            s: String::from_utf16_lossy(&b[start..end_b]),
        });
    }
    op
}

fn transform_prim(a: &TextPrim, b: &TextPrim, a_wins: bool) -> Vec<TextPrim> {
    match (a, b) {
        (TextPrim::Insert { p: ap, s: as_ }, TextPrim::Insert { p: bp, s: bs }) => {
            if *ap < *bp || (*ap == *bp && a_wins) {
                vec![a.clone()]
            } else {
                let blen = bs.encode_utf16().count();
                vec![TextPrim::Insert {
                    p: *ap + blen,
                    s: as_.clone(),
                }]
            }
        }
        (TextPrim::Insert { p: ap, s: as_ }, TextPrim::Delete { p: bp, n: bn }) => {
            if *ap <= *bp {
                vec![a.clone()]
            } else if *ap >= *bp + *bn {
                vec![TextPrim::Insert {
                    p: *ap - *bn,
                    s: as_.clone(),
                }]
            } else {
                vec![TextPrim::Insert {
                    p: *bp,
                    s: as_.clone(),
                }]
            }
        }
        (TextPrim::Delete { p: ap, n: an }, TextPrim::Insert { p: bp, s: bs }) => {
            let blen = bs.encode_utf16().count();
            if *bp <= *ap {
                vec![TextPrim::Delete {
                    p: *ap + blen,
                    n: *an,
                }]
            } else if *bp >= *ap + *an {
                vec![a.clone()]
            } else {
                let before = *bp - *ap;
                vec![
                    TextPrim::Delete { p: *ap, n: before },
                    TextPrim::Delete {
                        p: *ap + blen,
                        n: *an - before,
                    },
                ]
            }
        }
        (TextPrim::Delete { p: ap, n: an }, TextPrim::Delete { p: bp, n: bn }) => {
            if *ap + *an <= *bp {
                vec![a.clone()]
            } else if *ap >= *bp + *bn {
                vec![TextPrim::Delete {
                    p: *ap - *bn,
                    n: *an,
                }]
            } else {
                let overlap = (*ap + *an).min(*bp + *bn) - (*ap).max(*bp);
                let rem = *an - overlap;
                if rem > 0 {
                    vec![TextPrim::Delete {
                        p: (*ap).min(*bp),
                        n: rem,
                    }]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

pub fn transform_ops(a: &[TextPrim], b: &[TextPrim], a_wins: bool) -> (Vec<TextPrim>, Vec<TextPrim>) {
    if a.is_empty() || b.is_empty() {
        return (a.to_vec(), b.to_vec());
    }
    if a.len() > 1 {
        let (head, b1) = transform_ops(&a[0..1], b, a_wins);
        let (rest, b2) = transform_ops(&a[1..], &b1, a_wins);
        let mut out_a = head;
        out_a.extend(rest);
        return (out_a, b2);
    }
    if b.len() > 1 {
        let (a1, head) = transform_ops(a, &b[0..1], a_wins);
        let (a2, rest) = transform_ops(&a1, &b[1..], a_wins);
        let mut out_b = head;
        out_b.extend(rest);
        return (a2, out_b);
    }
    (
        transform_prim(&a[0], &b[0], a_wins),
        transform_prim(&b[0], &a[0], !a_wins),
    )
}

#[derive(Debug, Clone)]
pub struct TextSyncClient {
    pub text: String,
    pub version: u64,
    pub pending: Option<(String, Vec<TextPrim>)>,
    pub buffer: Option<Vec<TextPrim>>,
    op_counter: u64,
}

impl TextSyncClient {
    pub fn new(text: String, version: u64) -> Self {
        Self {
            text,
            version,
            pending: None,
            buffer: None,
            op_counter: 0,
        }
    }

    pub fn dirty(&self) -> bool {
        self.pending.is_some() || self.buffer.is_some()
    }

    pub fn edit(&mut self, tool_id: &str, next: &str) -> Option<Value> {
        let op = diff_text_utf16(&self.text, next);
        if op.is_empty() {
            return None;
        }
        self.text = next.to_string();
        match &mut self.buffer {
            Some(buf) => buf.extend(op),
            None => self.buffer = Some(op),
        }
        self.flush(tool_id)
    }

    pub fn receive(&mut self, tool_id: &str, op: &[TextPrim], version: u64, op_id: Option<&str>) -> (bool, Option<Value>) {
        if let Some((ref pid, _)) = self.pending {
            if op_id == Some(pid.as_str()) {
                self.version = version;
                self.pending = None;
                let next_cmd = self.flush(tool_id);
                return (false, next_cmd);
            }
        }
        if version != self.version + 1 {
            return (true, None); // out of step -> caller sends tool-sync
        }
        let mut incoming = op.to_vec();
        if let Some((pid, pend)) = self.pending.take() {
            let (new_pend, rest) = transform_ops(&pend, &incoming, false);
            self.pending = Some((pid, new_pend));
            incoming = rest;
        }
        if let Some(buf) = self.buffer.take() {
            let (new_buf, rest) = transform_ops(&buf, &incoming, false);
            self.buffer = Some(new_buf);
            incoming = rest;
        }
        self.version = version;
        self.text = apply_text_op(&self.text, &incoming);
        (false, None)
    }

    pub fn reset(&mut self, text: String, version: u64) {
        self.text = text;
        self.version = version;
        self.pending = None;
        self.buffer = None;
    }

    fn flush(&mut self, tool_id: &str) -> Option<Value> {
        if self.pending.is_some() {
            return None;
        }
        let buf = self.buffer.take()?;
        self.op_counter = self.op_counter.wrapping_add(1);
        let op_id = format!("rs-{}-{}", self.version, self.op_counter);
        let msg = json!({
            "type": "tool-text-op",
            "id": tool_id,
            "version": self.version,
            "op": buf,
            "opId": op_id,
        });
        self.pending = Some((op_id, buf));
        Some(msg)
    }
}

// ---------------------------------------------------------------------------
// Modelos das Ferramentas da Sala
// ---------------------------------------------------------------------------

pub const PANEL_TOOL_KINDS: &[(&str, &str, &str)] = &[
    // Mesma ordem, nomes e dicas do painel do site (`PANEL_KINDS` + `roomTools.kind*` em pt.json).
    ("whiteboard", "Lousa", "Desenhe e escreva juntos"),
    ("notepad", "Bloco de notas", "Texto editado por todos em tempo real"),
    ("code", "Editor de código", "Código com cores, editado junto"),
    ("sheet", "Planilha", "Células e fórmulas, como no Excel"),
    ("doc", "Documento", "Texto formatado, como no Word"),
    ("tasks", "Tarefas", "Lista de tarefas compartilhada"),
    ("wheel", "Roleta", "Sorteio girando para a sala toda ver"),
    ("calc", "Calculadora", "Contas que a sala inteira vê e faz junto"),
    ("annotate", "Anotar na tela", "Desenhe por cima da tela compartilhada"),
];

/// O recurso de plano que abrir cada tipo exige (espelho do `TOOL_FEATURES` do site e da API).
pub fn kind_feature(kind: &str) -> &'static str {
    match kind {
        "tasks" => "room_tools_tasks",
        "code" | "sheet" | "doc" => "room_tools_text",
        "whiteboard" | "annotate" => "room_tools_draw",
        _ => "room_tools_basic",
    }
}

/// O plano mostrado ao lado de um tipo pago (`planForKind` do site). `None` = gratis.
pub fn kind_plan_name(kind: &str) -> Option<&'static str> {
    match kind_feature(kind) {
        "room_tools_tasks" => Some("Pro"),
        "room_tools_text" => Some("Pro Max"),
        "room_tools_draw" => Some("Pro Ultra"),
        _ => None,
    }
}

/// `canOpenKind` do site: gratis, ou a conta tem o recurso (o servidor confere de novo).
pub fn can_open_kind(kind: &str, features: &[String]) -> bool {
    let f = kind_feature(kind);
    f == "room_tools_basic" || features.iter().any(|x| x == f)
}

pub const CODE_LANGUAGES: &[&str] = &[
    "plaintext",
    "javascript",
    "typescript",
    "python",
    "rust",
    "go",
    "cpp",
    "csharp",
    "java",
    "sql",
    "html",
    "css",
    "json",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStroke {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub by: String,
    pub shape: String,
    #[serde(default = "default_stroke_color", deserialize_with = "de_stroke_color")]
    pub color: String,
    #[serde(default = "default_stroke_width", deserialize_with = "de_stroke_width")]
    pub width: f32,
    #[serde(default)]
    pub points: Vec<f32>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    /// "Anotar na tela": a midia em que o traco foi feito (`screen:<id>`, `camera:<id>`...).
    #[serde(default)]
    pub target: Option<String>,
}

fn de_null_string<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

fn de_stroke_color<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    Ok(opt.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_stroke_color))
}

fn de_stroke_width<'de, D>(d: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<f32>::deserialize(d)?.unwrap_or_else(default_stroke_width))
}

fn de_null_bool<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(d)?.unwrap_or(false))
}

fn de_null_u64<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<u64>::deserialize(d)?.unwrap_or(0))
}

fn de_null_priority<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    Ok(match opt.as_deref() {
        Some("high") | Some("low") | Some("normal") => opt.unwrap(),
        _ => "normal".to_string(),
    })
}

fn de_wheel_color<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    Ok(opt.filter(|s| !s.trim().is_empty()).unwrap_or_else(default_wheel_color))
}

fn de_wheel_weight<'de, D>(d: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<f64>::deserialize(d)?.unwrap_or_else(default_weight).max(1.0))
}

fn default_stroke_color() -> String {
    "#ef4444".to_string()
}

fn default_stroke_width() -> f32 {
    4.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollOption {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub text: String,
    #[serde(default, deserialize_with = "de_null_bool")]
    pub done: bool,
    #[serde(default, deserialize_with = "de_null_string")]
    pub by_name: String,
    #[serde(default)]
    pub done_by_name: Option<String>,
    #[serde(default)]
    pub assignee_id: Option<String>,
    #[serde(default)]
    pub assignee_name: Option<String>,
    #[serde(default = "default_task_priority", deserialize_with = "de_null_priority")]
    pub priority: String,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default, deserialize_with = "de_null_u64")]
    pub created_at: u64,
}

fn default_task_priority() -> String {
    "normal".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRules {
    #[serde(default = "default_rule_managers", deserialize_with = "de_null_string")]
    pub add: String,
    #[serde(default = "default_rule_everyone", deserialize_with = "de_null_string")]
    pub complete: String,
    #[serde(default = "default_rule_managers", deserialize_with = "de_null_string")]
    pub edit: String,
}

fn default_rule_managers() -> String {
    "managers".to_string()
}

fn default_rule_everyone() -> String {
    "everyone".to_string()
}

/// Mesmo `DEFAULT_TASK_RULES` do site: criar/editar so administradores, concluir todos.
impl Default for TaskRules {
    fn default() -> Self {
        Self {
            add: default_rule_managers(),
            complete: default_rule_everyone(),
            edit: default_rule_managers(),
        }
    }
}

pub fn task_allows(rules: &TaskRules, action: &str, self_uid: Option<&str>, is_mgr: bool, item: Option<&TaskItem>) -> bool {
    if is_mgr {
        return true;
    }
    let rule = match action {
        "add" => rules.add.as_str(),
        "complete" => rules.complete.as_str(),
        "edit" => rules.edit.as_str(),
        _ => "managers",
    };
    if rule == "everyone" {
        return true;
    }
    if rule == "assignee" {
        return self_uid.is_some_and(|uid| item.and_then(|it| it.assignee_id.as_deref()) == Some(uid));
    }
    false
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelEntry {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub text: String,
    #[serde(default = "default_wheel_color", deserialize_with = "de_wheel_color")]
    pub color: String,
    #[serde(default = "default_weight", deserialize_with = "de_wheel_weight")]
    pub weight: f64,
}

fn default_wheel_color() -> String {
    "#10b981".to_string()
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Debug, Clone)]
pub struct WheelSpinState {
    pub id: String,
    pub winner_text: String,
    pub by_name: String,
    pub duration_secs: f32,
    pub from_deg: f32,
    pub to_deg: f32,
    pub entries: Vec<WheelEntry>,
    pub local_start: Instant,
}

pub const MEDIA_TOOL_KINDS: &[&str] = &[
    "whiteboard",
    "notepad",
    "code",
    "wheel",
    "calc",
    "sheet",
    "doc",
];

pub fn is_media_tool_kind(kind: &str) -> bool {
    MEDIA_TOOL_KINDS.contains(&kind)
}

pub const REACTION_EMOJIS: &[&str] = &[
    "👍", "❤️", "😂", "😮", "😢", "👏", "🔥", "🎉", "💯", "🤔", "👀", "🙏",
];

#[derive(Debug, Clone)]
pub struct FloatingReaction {
    pub id: String,
    pub target: String,
    pub emoji: String,
    pub name: String,
    pub x_frac: f32,
    pub created_at: Instant,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SheetCell {
    #[serde(default)]
    pub v: String,
    #[serde(default)]
    pub b: bool,
    #[serde(default)]
    pub i: bool,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub align: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelResult {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub text: String,
    #[serde(default, deserialize_with = "de_wheel_color")]
    pub color: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub by_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalcEntry {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub expr: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub result: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub by_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaisedHand {
    pub id: String,
    #[serde(default, deserialize_with = "de_null_string")]
    pub name: String,
    #[serde(default, deserialize_with = "de_null_u64")]
    pub at: u64,
}

#[derive(Debug, Clone)]
pub enum ToolPayload {
    Whiteboard {
        strokes: Vec<ToolStroke>,
    },
    Annotate {
        strokes: Vec<ToolStroke>,
    },
    Text {
        text: String,
        version: u64,
        language: String,
    },
    Tasks {
        items: Vec<TaskItem>,
        rules: TaskRules,
    },
    Poll {
        question: String,
        options: Vec<PollOption>,
        multi: bool,
        closed: bool,
        counts: Vec<u64>,
        total_voters: u64,
        mine: Vec<String>,
    },
    Wheel {
        entries: Vec<WheelEntry>,
        spin: Option<WheelSpinState>,
        history: Vec<WheelResult>,
        remove_winner: bool,
        announce: bool,
        duration: u32,
        spin_access: String,
        edit_access: String,
    },
    Calc {
        expr: String,
        history: Vec<CalcEntry>,
        memory: f64,
        angle: String,
    },
    Sheet {
        rows: usize,
        cols: usize,
        cells: HashMap<String, SheetCell>,
        col_widths: HashMap<usize, u32>,
    },
    Reactions,
}

#[derive(Debug, Clone)]
pub struct RoomTool {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub created_by_id: String,
    pub created_by_name: String,
    pub access: String,
    pub allowed: Vec<String>,
    pub payload: ToolPayload,
}

impl RoomTool {
    pub fn from_json(v: &Value) -> Option<Self> {
        Self::from_json_with_starts(v, None)
    }

    pub fn from_json_with_starts(
        v: &Value,
        spin_starts: Option<&mut HashMap<String, Instant>>,
    ) -> Option<Self> {
        let id = v.get("id")?.as_str()?.to_string();
        let kind = v.get("kind")?.as_str()?.to_string();
        let title = v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or(&kind)
            .to_string();
        let created_by_id = v
            .get("createdById")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let created_by_name = v
            .get("createdByName")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let access = v
            .get("access")
            .and_then(|s| s.as_str())
            .unwrap_or("everyone")
            .to_string();
        let allowed = v
            .get("allowed")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let payload = match kind.as_str() {
            "whiteboard" => ToolPayload::Whiteboard {
                strokes: parse_strokes(v.get("strokes")),
            },
            "annotate" => ToolPayload::Annotate {
                strokes: parse_strokes(v.get("strokes")),
            },
            "notepad" | "code" | "doc" => ToolPayload::Text {
                text: v
                    .get("text")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                version: v.get("version").and_then(|n| n.as_u64()).unwrap_or(0),
                language: v
                    .get("language")
                    .and_then(|s| s.as_str())
                    .unwrap_or("plaintext")
                    .to_string(),
            },
            "tasks" => ToolPayload::Tasks {
                items: v
                    .get("items")
                    .and_then(|arr| serde_json::from_value(arr.clone()).ok())
                    .unwrap_or_default(),
                rules: v
                    .get("rules")
                    .and_then(|r| serde_json::from_value(r.clone()).ok())
                    .unwrap_or_default(),
            },
            "poll" => ToolPayload::Poll {
                question: v
                    .get("question")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                options: v
                    .get("options")
                    .and_then(|arr| serde_json::from_value(arr.clone()).ok())
                    .unwrap_or_default(),
                multi: v.get("multi").and_then(|b| b.as_bool()).unwrap_or(false),
                closed: v.get("closed").and_then(|b| b.as_bool()).unwrap_or(false),
                counts: v
                    .get("counts")
                    .and_then(|arr| {
                        arr.as_array()
                            .map(|xs| xs.iter().filter_map(|x| x.as_u64()).collect())
                    })
                    .unwrap_or_default(),
                total_voters: v.get("totalVoters").and_then(|n| n.as_u64()).unwrap_or(0),
                mine: v
                    .get("mine")
                    .and_then(|arr| {
                        arr.as_array()
                            .map(|xs| xs.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    })
                    .unwrap_or_default(),
            },
            "wheel" => ToolPayload::Wheel {
                entries: v
                    .get("entries")
                    .and_then(|arr| serde_json::from_value(arr.clone()).ok())
                    .unwrap_or_default(),
                spin: parse_wheel_spin(v.get("spin"), spin_starts),
                history: v
                    .get("history")
                    .and_then(|arr| serde_json::from_value(arr.clone()).ok())
                    .unwrap_or_default(),
                remove_winner: v
                    .get("settings")
                    .and_then(|s| s.get("removeWinner"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false),
                announce: v
                    .get("settings")
                    .and_then(|s| s.get("announce"))
                    .and_then(|b| b.as_bool())
                    .unwrap_or(true),
                duration: v
                    .get("settings")
                    .and_then(|s| s.get("duration"))
                    .and_then(|n| n.as_u64())
                    .unwrap_or(5) as u32,
                spin_access: v
                    .get("settings")
                    .and_then(|s| s.get("spin"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("everyone")
                    .to_string(),
                edit_access: v
                    .get("settings")
                    .and_then(|s| s.get("edit"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("everyone")
                    .to_string(),
            },
            "calc" => ToolPayload::Calc {
                expr: v
                    .get("expr")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                history: v
                    .get("history")
                    .and_then(|arr| serde_json::from_value(arr.clone()).ok())
                    .unwrap_or_default(),
                memory: v.get("memory").and_then(|n| n.as_f64()).unwrap_or(0.0),
                angle: v
                    .get("angle")
                    .and_then(|s| s.as_str())
                    .unwrap_or("deg")
                    .to_string(),
            },
            "sheet" => {
                let mut col_widths = HashMap::new();
                if let Some(obj) = v.get("colWidths").and_then(|x| x.as_object()) {
                    for (k, wv) in obj {
                        if let (Ok(c), Some(w)) = (k.parse::<usize>(), wv.as_u64()) {
                            col_widths.insert(c, (w as u32).clamp(40, 500));
                        }
                    }
                }
                ToolPayload::Sheet {
                    rows: v
                        .get("rows")
                        .and_then(|n| n.as_u64())
                        .unwrap_or(20)
                        .clamp(1, 500) as usize,
                    cols: v
                        .get("cols")
                        .and_then(|n| n.as_u64())
                        .unwrap_or(10)
                        .clamp(1, 52) as usize,
                    cells: parse_sheet_cells(v.get("cells")),
                    col_widths,
                }
            }
            "reactions" => ToolPayload::Reactions,
            _ => return None,
        };

        Some(Self {
            id,
            kind,
            title,
            created_by_id,
            created_by_name,
            access,
            allowed,
            payload,
        })
    }
}

fn parse_strokes(val: Option<&Value>) -> Vec<ToolStroke> {
    val.and_then(|arr| serde_json::from_value::<Vec<ToolStroke>>(arr.clone()).ok())
        .unwrap_or_default()
}

fn parse_wheel_spin(
    val: Option<&Value>,
    spin_starts: Option<&mut HashMap<String, Instant>>,
) -> Option<WheelSpinState> {
    let v = val?.as_object()?;
    let id = v.get("id")?.as_str()?.to_string();
    let winner_text = v
        .get("winnerText")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let by_name = v
        .get("byName")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    // O servidor envia duration em milissegundos (ex: 5000); aceita segundos legados <= 60.
    let raw_dur = v.get("duration").and_then(|n| n.as_f64()).unwrap_or(5000.0);
    let duration_secs = (if raw_dur > 60.0 {
        raw_dur / 1000.0
    } else {
        raw_dur
    }) as f32;
    let from_deg = v.get("from").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
    let to_deg = v.get("to").and_then(|n| n.as_f64()).unwrap_or(360.0) as f32;
    let entries = v
        .get("entries")
        .and_then(|arr| serde_json::from_value::<Vec<WheelEntry>>(arr.clone()).ok())
        .unwrap_or_default();
    let elapsed_ms = v.get("elapsed").and_then(|n| n.as_u64()).unwrap_or(0);
    let computed_start = Instant::now()
        .checked_sub(std::time::Duration::from_millis(elapsed_ms))
        .unwrap_or_else(Instant::now);
    let local_start = match spin_starts {
        Some(map) => *map.entry(id.clone()).or_insert(computed_start),
        None => computed_start,
    };

    Some(WheelSpinState {
        id,
        winner_text,
        by_name,
        duration_secs: duration_secs.clamp(0.5, 30.0),
        from_deg,
        to_deg,
        entries,
        local_start,
    })
}

pub fn wheel_angle_at(spin: &WheelSpinState, now: Instant) -> (f32, bool) {
    let dur = spin.duration_secs.max(0.5);
    let elapsed = now.saturating_duration_since(spin.local_start).as_secs_f32();
    if elapsed >= dur {
        return (spin.to_deg, false);
    }
    let t = (elapsed / dur).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - t).powi(4);
    (spin.from_deg + (spin.to_deg - spin.from_deg) * ease, true)
}

pub fn sheet_cell_key(row: usize, col: usize) -> String {
    format!("{},{}", row, col)
}

pub fn sheet_col_name(mut col: usize) -> String {
    let mut name = String::new();
    let mut n = col + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        name.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    col = 0;
    let _ = col;
    name
}

pub fn sheet_cell_name(row: usize, col: usize) -> String {
    format!("{}{}", sheet_col_name(col), row + 1)
}

pub fn parse_sheet_coord(s: &str) -> Option<(usize, usize)> {
    let t = s.trim();
    if let Some((rs, cs)) = t.split_once(',') {
        let r = rs.trim().parse::<usize>().ok()?;
        let c = cs.trim().parse::<usize>().ok()?;
        return Some((r, c));
    }
    let mut split_at = 0usize;
    for (i, ch) in t.char_indices() {
        if ch.is_ascii_alphabetic() {
            split_at = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if split_at == 0 || split_at == t.len() {
        return None;
    }
    let col_str = t[..split_at].to_ascii_uppercase();
    let row_num = t[split_at..].parse::<usize>().ok()?;
    if row_num == 0 {
        return None;
    }
    let mut col = 0usize;
    for b in col_str.bytes() {
        col = col * 26 + ((b - b'A') as usize + 1);
    }
    Some((row_num - 1, col - 1))
}

fn sheet_cell_is_empty(c: &SheetCell) -> bool {
    c.v.is_empty() && !c.b && !c.i && c.bg.is_none() && c.color.is_none() && c.align.is_none()
}

fn parse_sheet_cells(val: Option<&Value>) -> HashMap<String, SheetCell> {
    let mut map = HashMap::new();
    let Some(obj) = val.and_then(|v| v.as_object()) else {
        return map;
    };
    for (k, raw_cell) in obj {
        let key = parse_sheet_coord(k)
            .map(|(r, c)| sheet_cell_key(r, c))
            .unwrap_or_else(|| k.clone());
        if let Some(s) = raw_cell.as_str() {
            if !s.is_empty() {
                map.insert(
                    key,
                    SheetCell {
                        v: s.to_string(),
                        ..Default::default()
                    },
                );
            }
        } else if let Some(co) = raw_cell.as_object() {
            let v = co
                .get("v")
                .or_else(|| co.get("raw"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let cell = SheetCell {
                v,
                b: co.get("b").and_then(|x| x.as_bool()).unwrap_or(false),
                i: co.get("i").and_then(|x| x.as_bool()).unwrap_or(false),
                bg: co.get("bg").and_then(|x| x.as_str()).map(str::to_string),
                color: co.get("color").and_then(|x| x.as_str()).map(str::to_string),
                align: co.get("align").and_then(|x| x.as_str()).map(str::to_string),
            };
            if !sheet_cell_is_empty(&cell) {
                map.insert(key, cell);
            }
        }
    }
    map
}

pub fn apply_sheet_change(
    rows: &mut usize,
    cols: &mut usize,
    cells: &mut HashMap<String, SheetCell>,
    col_widths: &mut HashMap<usize, u32>,
    change: &Value,
) {
    let op = change
        .get("op")
        .or_else(|| change.get("type"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    match op {
        "set" | "cell" => {
            if let (Some(cell_ref), Some(val)) = (
                change
                    .get("cell")
                    .or_else(|| change.get("ref"))
                    .and_then(|s| s.as_str()),
                change
                    .get("v")
                    .or_else(|| change.get("raw"))
                    .or_else(|| change.get("value"))
                    .and_then(|s| s.as_str()),
            ) {
                if let Some((r, c)) = parse_sheet_coord(cell_ref) {
                    if r < *rows && c < *cols {
                        let k = sheet_cell_key(r, c);
                        let mut next = cells.get(&k).cloned().unwrap_or_default();
                        next.v = val.to_string();
                        if sheet_cell_is_empty(&next) {
                            cells.remove(&k);
                        } else {
                            cells.insert(k, next);
                        }
                    }
                }
            }
        }
        "fill" => {
            if let (Some(start_ref), Some(matrix)) = (
                change.get("cell").and_then(|s| s.as_str()),
                change.get("values").and_then(|a| a.as_array()),
            ) {
                if let Some((sr, sc)) = parse_sheet_coord(start_ref) {
                    for (dr, row_val) in matrix.iter().enumerate() {
                        if let Some(arr) = row_val.as_array() {
                            for (dc, v) in arr.iter().enumerate() {
                                if let Some(s) = v.as_str() {
                                    let r = sr + dr;
                                    let c = sc + dc;
                                    if r < *rows && c < *cols {
                                        let k = sheet_cell_key(r, c);
                                        let mut next = cells.get(&k).cloned().unwrap_or_default();
                                        next.v = s.to_string();
                                        if sheet_cell_is_empty(&next) {
                                            cells.remove(&k);
                                        } else {
                                            cells.insert(k, next);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "style" => {
            if let (Some(keys), Some(style)) = (
                change.get("cells").and_then(|a| a.as_array()),
                change.get("style").and_then(|o| o.as_object()),
            ) {
                for kv in keys {
                    if let Some(kstr) = kv.as_str() {
                        if let Some((r, c)) = parse_sheet_coord(kstr) {
                            if r < *rows && c < *cols {
                                let k = sheet_cell_key(r, c);
                                let mut next = cells.get(&k).cloned().unwrap_or_default();
                                if let Some(v) = style.get("b") {
                                    next.b = v.as_bool().unwrap_or(false);
                                }
                                if let Some(v) = style.get("i") {
                                    next.i = v.as_bool().unwrap_or(false);
                                }
                                if let Some(v) = style.get("bg") {
                                    next.bg = v.as_str().map(str::to_string);
                                }
                                if let Some(v) = style.get("color") {
                                    next.color = v.as_str().map(str::to_string);
                                }
                                if let Some(v) = style.get("align") {
                                    next.align = v.as_str().map(str::to_string);
                                }
                                if sheet_cell_is_empty(&next) {
                                    cells.remove(&k);
                                } else {
                                    cells.insert(k, next);
                                }
                            }
                        }
                    }
                }
            }
        }
        "size" => {
            let nr = change
                .get("rows")
                .and_then(|n| n.as_u64())
                .map(|n| (n as usize).clamp(1, 500));
            let nc = change
                .get("cols")
                .and_then(|n| n.as_u64())
                .map(|n| (n as usize).clamp(1, 52));
            if let Some(r) = nr {
                *rows = r;
            }
            if let Some(c) = nc {
                *cols = c;
            }
            let max_r = *rows;
            let max_c = *cols;
            cells.retain(|k, _| {
                parse_sheet_coord(k)
                    .is_some_and(|(r, c)| r < max_r && c < max_c)
            });
            col_widths.retain(|&c, _| c < max_c);
        }
        "width" => {
            if let (Some(col), Some(w)) = (
                change.get("col").and_then(|n| n.as_u64()).map(|n| n as usize),
                change.get("width").and_then(|n| n.as_u64()).map(|n| (n as u32).clamp(40, 500)),
            ) {
                if col < *cols {
                    col_widths.insert(col, w);
                }
            }
        }
        "insert" => {
            let axis = change.get("axis").and_then(|s| s.as_str()).unwrap_or("");
            let at = change.get("at").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
            if axis == "row" && *rows < 500 {
                *rows += 1;
                let old = std::mem::take(cells);
                for (k, v) in old {
                    if let Some((r, c)) = parse_sheet_coord(&k) {
                        let nr = if r >= at { r + 1 } else { r };
                        cells.insert(sheet_cell_key(nr, c), v);
                    }
                }
            } else if axis == "col" && *cols < 52 {
                *cols += 1;
                let old = std::mem::take(cells);
                for (k, v) in old {
                    if let Some((r, c)) = parse_sheet_coord(&k) {
                        let nc = if c >= at { c + 1 } else { c };
                        cells.insert(sheet_cell_key(r, nc), v);
                    }
                }
                let old_w = std::mem::take(col_widths);
                for (c, w) in old_w {
                    col_widths.insert(if c >= at { c + 1 } else { c }, w);
                }
            }
        }
        "delete" => {
            let axis = change.get("axis").and_then(|s| s.as_str()).unwrap_or("");
            let at = change.get("at").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
            if axis == "row" && *rows > 1 {
                *rows -= 1;
                let old = std::mem::take(cells);
                for (k, v) in old {
                    if let Some((r, c)) = parse_sheet_coord(&k) {
                        if r == at {
                            continue;
                        }
                        let nr = if r > at { r - 1 } else { r };
                        cells.insert(sheet_cell_key(nr, c), v);
                    }
                }
            } else if axis == "col" && *cols > 1 {
                *cols -= 1;
                let old = std::mem::take(cells);
                for (k, v) in old {
                    if let Some((r, c)) = parse_sheet_coord(&k) {
                        if c == at {
                            continue;
                        }
                        let nc = if c > at { c - 1 } else { c };
                        cells.insert(sheet_cell_key(r, nc), v);
                    }
                }
                let old_w = std::mem::take(col_widths);
                for (c, w) in old_w {
                    if c == at {
                        continue;
                    }
                    col_widths.insert(if c > at { c - 1 } else { c }, w);
                }
            }
        }
        _ => {}
    }
}

pub const SHEET_DEFAULT_COL_WIDTH: u32 = 96;

#[derive(Debug, Clone)]
enum SheetVal {
    Num(f64),
    Str(String),
    Bool(bool),
    Range(Vec<SheetVal>),
}

fn sheet_literal_val(raw: &str) -> SheetVal {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SheetVal::Str(String::new());
    }
    // "1.234,56" (PT-BR) e "1234.56" -> numero
    let norm = if trimmed.contains(',') && trimmed.contains('.') {
        trimmed.replace('.', "").replace(',', ".")
    } else {
        trimmed.replace(',', ".")
    };
    if let Ok(n) = norm.parse::<f64>() {
        if n.is_finite() {
            return SheetVal::Num(n);
        }
    }
    SheetVal::Str(raw.to_string())
}

fn sheet_single(v: SheetVal) -> Result<SheetVal, &'static str> {
    match v {
        SheetVal::Range(mut arr) => {
            if arr.len() == 1 {
                Ok(arr.remove(0))
            } else {
                Err("#VALOR!")
            }
        }
        other => Ok(other),
    }
}

fn sheet_to_number(v: SheetVal) -> Result<f64, &'static str> {
    match sheet_single(v)? {
        SheetVal::Num(n) => Ok(n),
        SheetVal::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
        SheetVal::Str(s) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(0.0);
            }
            let norm = if t.contains(',') && t.contains('.') {
                t.replace('.', "").replace(',', ".")
            } else {
                t.replace(',', ".")
            };
            norm.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or("#VALOR!")
        }
        SheetVal::Range(_) => Err("#VALOR!"),
    }
}

fn sheet_to_text(v: SheetVal) -> Result<String, &'static str> {
    match sheet_single(v)? {
        SheetVal::Num(n) => Ok(format_calc_number(n)),
        SheetVal::Bool(b) => Ok(if b { "VERDADEIRO" } else { "FALSO" }.to_string()),
        SheetVal::Str(s) => Ok(s),
        SheetVal::Range(_) => Err("#VALOR!"),
    }
}

fn sheet_to_bool(v: SheetVal) -> Result<bool, &'static str> {
    match sheet_single(v)? {
        SheetVal::Bool(b) => Ok(b),
        SheetVal::Num(n) => Ok(n != 0.0),
        SheetVal::Str(s) => match s.trim().to_ascii_uppercase().as_str() {
            "VERDADEIRO" | "TRUE" => Ok(true),
            "FALSO" | "FALSE" | "" => Ok(false),
            _ => Err("#VALOR!"),
        },
        SheetVal::Range(_) => Err("#VALOR!"),
    }
}

fn sheet_flat(args: Vec<SheetVal>) -> Vec<SheetVal> {
    let mut out = Vec::new();
    for a in args {
        match a {
            SheetVal::Range(v) => out.extend(v),
            other => out.push(other),
        }
    }
    out
}

fn sheet_numbers(args: Vec<SheetVal>) -> Result<Vec<f64>, &'static str> {
    let mut out = Vec::new();
    for a in args {
        match a {
            SheetVal::Range(arr) => {
                for item in arr {
                    if let SheetVal::Num(n) = item {
                        out.push(n);
                    }
                }
            }
            other => out.push(sheet_to_number(other)?),
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum SheetTok {
    Num(f64),
    Str(String),
    Ref(String),
    Range(String, String),
    Name(String),
    Op(String),
    LParen,
    RParen,
    Sep,
}

fn parse_ref_token(s: &str) -> Option<(usize, usize)> {
    let clean: String = s.chars().filter(|&c| c != '$').collect();
    let alpha_len = clean.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if alpha_len == 0 || alpha_len > 2 || alpha_len == clean.len() {
        return None;
    }
    let digits = &clean[alpha_len..];
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let row = digits.parse::<usize>().ok().and_then(|n| n.checked_sub(1))?;
    let mut col = 0usize;
    for b in clean[..alpha_len].to_ascii_uppercase().bytes() {
        col = col * 26 + ((b - b'A') as usize + 1);
    }
    Some((row, col - 1))
}

fn tokenize_sheet_formula(src: &str) -> Result<Vec<SheetTok>, &'static str> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '"' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() {
                if chars[i] == '"' && i + 1 < chars.len() && chars[i + 1] == '"' {
                    s.push('"');
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            toks.push(SheetTok::Str(s));
            continue;
        }
        if ch.is_ascii_digit() {
            let start = i;
            let mut seen_sep = false;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (!seen_sep && (chars[i] == '.' || chars[i] == ',')))
            {
                if chars[i] == '.' || chars[i] == ',' {
                    seen_sep = true;
                }
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            let n = raw.replace(',', ".").parse::<f64>().map_err(|_| "#VALOR!")?;
            toks.push(SheetTok::Num(n));
            continue;
        }
        if ch == '$' || ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.' || chars[i] == '$')
            {
                i += 1;
            }
            let first: String = chars[start..i].iter().collect();
            if i < chars.len() && chars[i] == ':' {
                i += 1;
                let s2 = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '$') {
                    i += 1;
                }
                let second: String = chars[s2..i].iter().collect();
                if parse_ref_token(&first).is_some() && parse_ref_token(&second).is_some() {
                    toks.push(SheetTok::Range(first, second));
                    continue;
                }
                return Err("#REF!");
            }
            if parse_ref_token(&first).is_some() {
                toks.push(SheetTok::Ref(first));
            } else {
                toks.push(SheetTok::Name(first.to_uppercase()));
            }
            continue;
        }
        if ch == '(' {
            toks.push(SheetTok::LParen);
            i += 1;
        } else if ch == ')' {
            toks.push(SheetTok::RParen);
            i += 1;
        } else if ch == ';' || ch == ',' {
            toks.push(SheetTok::Sep);
            i += 1;
        } else if i + 1 < chars.len()
            && matches!(
                (ch, chars[i + 1]),
                ('<', '=') | ('>', '=') | ('<', '>')
            )
        {
            toks.push(SheetTok::Op(format!("{}{}", ch, chars[i + 1])));
            i += 2;
        } else if "+-*/^&=<>%".contains(ch) {
            toks.push(SheetTok::Op(ch.to_string()));
            i += 1;
        } else {
            return Err("#NOME?");
        }
    }
    Ok(toks)
}

struct SheetFormulaParser<'a> {
    toks: Vec<SheetTok>,
    pos: usize,
    cells: &'a HashMap<String, SheetCell>,
    visiting: &'a mut HashSet<(usize, usize)>,
}

impl<'a> SheetFormulaParser<'a> {
    fn peek(&self) -> Option<&SheetTok> {
        self.toks.get(self.pos)
    }

    fn eval_cell(&mut self, r: usize, c: usize) -> Result<SheetVal, &'static str> {
        if r >= 500 || c >= 52 {
            return Err("#REF!");
        }
        eval_sheet_val_inner(self.cells, r, c, self.visiting)
    }

    fn parse_cmp(&mut self) -> Result<SheetVal, &'static str> {
        let mut left = self.parse_concat()?;
        while let Some(SheetTok::Op(op)) = self.peek().cloned() {
            if !matches!(op.as_str(), "=" | "<>" | "<" | "<=" | ">" | ">=") {
                break;
            }
            self.pos += 1;
            let right = self.parse_concat()?;
            let a = sheet_single(left)?;
            let b = sheet_single(right)?;
            let res = match (&a, &b) {
                (SheetVal::Num(x), SheetVal::Num(y)) => match op.as_str() {
                    "=" => (x - y).abs() < 1e-12,
                    "<>" => (x - y).abs() >= 1e-12,
                    "<" => x < y,
                    "<=" => x <= y,
                    ">" => x > y,
                    _ => x >= y,
                },
                (SheetVal::Bool(x), SheetVal::Bool(y)) => match op.as_str() {
                    "=" => x == y,
                    "<>" => x != y,
                    "<" => !x && *y,
                    "<=" => x <= y,
                    ">" => *x && !y,
                    _ => x >= y,
                },
                _ => {
                    let sa = sheet_to_text(a)?.to_lowercase();
                    let sb = sheet_to_text(b)?.to_lowercase();
                    match op.as_str() {
                        "=" => sa == sb,
                        "<>" => sa != sb,
                        "<" => sa < sb,
                        "<=" => sa <= sb,
                        ">" => sa > sb,
                        _ => sa >= sb,
                    }
                }
            };
            left = SheetVal::Bool(res);
        }
        Ok(left)
    }

    fn parse_concat(&mut self) -> Result<SheetVal, &'static str> {
        let mut left = self.parse_add()?;
        while matches!(self.peek(), Some(SheetTok::Op(op)) if op == "&") {
            self.pos += 1;
            let right = self.parse_add()?;
            let sa = sheet_to_text(left)?;
            let sb = sheet_to_text(right)?;
            left = SheetVal::Str(format!("{}{}", sa, sb));
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<SheetVal, &'static str> {
        let mut left = self.parse_mul()?;
        while let Some(SheetTok::Op(op)) = self.peek().cloned() {
            if op != "+" && op != "-" {
                break;
            }
            self.pos += 1;
            let right = self.parse_mul()?;
            let a = sheet_to_number(left)?;
            let b = sheet_to_number(right)?;
            left = SheetVal::Num(if op == "+" { a + b } else { a - b });
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<SheetVal, &'static str> {
        let mut left = self.parse_pow()?;
        while let Some(SheetTok::Op(op)) = self.peek().cloned() {
            if op != "*" && op != "/" {
                break;
            }
            self.pos += 1;
            let right = self.parse_pow()?;
            let a = sheet_to_number(left)?;
            let b = sheet_to_number(right)?;
            if op == "/" {
                if b.abs() < 1e-18 {
                    return Err("#DIV/0!");
                }
                left = SheetVal::Num(a / b);
            } else {
                left = SheetVal::Num(a * b);
            }
        }
        Ok(left)
    }

    fn parse_pow(&mut self) -> Result<SheetVal, &'static str> {
        let base = self.parse_unary()?;
        if matches!(self.peek(), Some(SheetTok::Op(op)) if op == "^") {
            self.pos += 1;
            let exp = self.parse_pow()?;
            return Ok(SheetVal::Num(sheet_to_number(base)?.powf(sheet_to_number(exp)?)));
        }
        Ok(base)
    }

    fn parse_unary(&mut self) -> Result<SheetVal, &'static str> {
        if let Some(SheetTok::Op(op)) = self.peek().cloned() {
            if op == "-" {
                self.pos += 1;
                return Ok(SheetVal::Num(-sheet_to_number(self.parse_unary()?)?));
            }
            if op == "+" {
                self.pos += 1;
                return self.parse_unary();
            }
        }
        let mut v = self.parse_primary()?;
        if matches!(self.peek(), Some(SheetTok::Op(op)) if op == "%") {
            self.pos += 1;
            v = SheetVal::Num(sheet_to_number(v)? / 100.0);
        }
        Ok(v)
    }

    fn parse_primary(&mut self) -> Result<SheetVal, &'static str> {
        let tok = self.toks.get(self.pos).cloned().ok_or("#VALOR!")?;
        self.pos += 1;
        match tok {
            SheetTok::Num(n) => Ok(SheetVal::Num(n)),
            SheetTok::Str(s) => Ok(SheetVal::Str(s)),
            SheetTok::Ref(r) => {
                let (rr, cc) = parse_ref_token(&r).ok_or("#REF!")?;
                self.eval_cell(rr, cc)
            }
            SheetTok::Range(a, b) => {
                let (r1, c1) = parse_ref_token(&a).ok_or("#REF!")?;
                let (r2, c2) = parse_ref_token(&b).ok_or("#REF!")?;
                let mut out = Vec::new();
                for r in r1.min(r2)..=r1.max(r2) {
                    for c in c1.min(c2)..=c1.max(c2) {
                        out.push(self.eval_cell(r, c)?);
                    }
                }
                Ok(SheetVal::Range(out))
            }
            SheetTok::Name(name) => {
                if matches!(self.peek(), Some(SheetTok::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(SheetTok::RParen)) {
                        loop {
                            args.push(self.parse_cmp()?);
                            if matches!(self.peek(), Some(SheetTok::Sep)) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if !matches!(self.peek(), Some(SheetTok::RParen)) {
                        return Err("#VALOR!");
                    }
                    self.pos += 1;
                    eval_sheet_fn(&name, args)
                } else if name == "VERDADEIRO" || name == "TRUE" {
                    Ok(SheetVal::Bool(true))
                } else if name == "FALSO" || name == "FALSE" {
                    Ok(SheetVal::Bool(false))
                } else {
                    Err("#NOME?")
                }
            }
            SheetTok::LParen => {
                let v = self.parse_cmp()?;
                if !matches!(self.peek(), Some(SheetTok::RParen)) {
                    return Err("#VALOR!");
                }
                self.pos += 1;
                Ok(v)
            }
            _ => Err("#VALOR!"),
        }
    }
}

fn eval_sheet_fn(name: &str, args: Vec<SheetVal>) -> Result<SheetVal, &'static str> {
    match name {
        "SOMA" | "SUM" => Ok(SheetVal::Num(sheet_numbers(args)?.into_iter().sum())),
        "MEDIA" | "MÉDIA" | "AVG" | "AVERAGE" => {
            let n = sheet_numbers(args)?;
            if n.is_empty() {
                return Err("#DIV/0!");
            }
            Ok(SheetVal::Num(n.iter().sum::<f64>() / n.len() as f64))
        }
        "MIN" | "MÍNIMO" | "MINIMO" => {
            let n = sheet_numbers(args)?;
            Ok(SheetVal::Num(
                n.into_iter().reduce(f64::min).unwrap_or(0.0),
            ))
        }
        "MAX" | "MÁXIMO" | "MAXIMO" => {
            let n = sheet_numbers(args)?;
            Ok(SheetVal::Num(
                n.into_iter().reduce(f64::max).unwrap_or(0.0),
            ))
        }
        "CONT.NÚM" | "CONT.NUM" | "COUNT" => {
            let cnt = sheet_flat(args)
                .into_iter()
                .filter(|v| matches!(v, SheetVal::Num(_)))
                .count();
            Ok(SheetVal::Num(cnt as f64))
        }
        "CONT.VALORES" | "COUNTA" => {
            let cnt = sheet_flat(args)
                .into_iter()
                .filter(|v| !matches!(v, SheetVal::Str(s) if s.is_empty()))
                .count();
            Ok(SheetVal::Num(cnt as f64))
        }
        "CONTAR.VAZIO" | "COUNTBLANK" => {
            let cnt = sheet_flat(args)
                .into_iter()
                .filter(|v| matches!(v, SheetVal::Str(s) if s.is_empty()))
                .count();
            Ok(SheetVal::Num(cnt as f64))
        }
        "ARRED" | "ROUND" => {
            let x = sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?;
            let digits = match args.get(1).cloned() {
                Some(v) => sheet_to_number(v)? as i32,
                None => 0,
            };
            let factor = 10_f64.powi(digits);
            Ok(SheetVal::Num((x * factor).round() / factor))
        }
        "ABS" => Ok(SheetVal::Num(
            sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?.abs(),
        )),
        "RAIZ" | "SQRT" => {
            let x = sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?;
            if x < 0.0 {
                return Err("#VALOR!");
            }
            Ok(SheetVal::Num(x.sqrt()))
        }
        "POTÊNCIA" | "POTENCIA" | "POWER" => {
            let b = sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?;
            let e = sheet_to_number(args.get(1).cloned().ok_or("#VALOR!")?)?;
            Ok(SheetVal::Num(b.powf(e)))
        }
        "INT" => Ok(SheetVal::Num(
            sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?.floor(),
        )),
        "MOD" => {
            let n = sheet_to_number(args.first().cloned().ok_or("#VALOR!")?)?;
            let d = sheet_to_number(args.get(1).cloned().ok_or("#VALOR!")?)?;
            if d.abs() < 1e-18 {
                return Err("#DIV/0!");
            }
            Ok(SheetVal::Num(n - d * (n / d).floor()))
        }
        "SE" | "IF" => {
            let cond = sheet_to_bool(args.first().cloned().ok_or("#VALOR!")?)?;
            if cond {
                Ok(args.get(1).cloned().unwrap_or(SheetVal::Bool(true)))
            } else {
                Ok(args.get(2).cloned().unwrap_or(SheetVal::Bool(false)))
            }
        }
        "E" | "AND" => {
            let mut all = true;
            for item in sheet_flat(args) {
                if !sheet_to_bool(item)? {
                    all = false;
                }
            }
            Ok(SheetVal::Bool(all))
        }
        "OU" | "OR" => {
            let mut any = false;
            for item in sheet_flat(args) {
                if sheet_to_bool(item)? {
                    any = true;
                }
            }
            Ok(SheetVal::Bool(any))
        }
        "NÃO" | "NAO" | "NOT" => Ok(SheetVal::Bool(!sheet_to_bool(
            args.first().cloned().ok_or("#VALOR!")?,
        )?)),
        "CONCATENAR" | "CONCAT" => {
            let mut out = String::new();
            for item in sheet_flat(args) {
                out.push_str(&sheet_to_text(item)?);
            }
            Ok(SheetVal::Str(out))
        }
        "MAIÚSCULA" | "MAIUSCULA" | "UPPER" => Ok(SheetVal::Str(
            sheet_to_text(args.first().cloned().ok_or("#VALOR!")?)?.to_uppercase(),
        )),
        "MINÚSCULA" | "MINUSCULA" | "LOWER" => Ok(SheetVal::Str(
            sheet_to_text(args.first().cloned().ok_or("#VALOR!")?)?.to_lowercase(),
        )),
        "ARRUMAR" | "TRIM" => {
            let t = sheet_to_text(args.first().cloned().ok_or("#VALOR!")?)?;
            let cleaned = t.split_whitespace().collect::<Vec<_>>().join(" ");
            Ok(SheetVal::Str(cleaned))
        }
        _ => Err("#NOME?"),
    }
}

fn eval_sheet_val_inner(
    cells: &HashMap<String, SheetCell>,
    row: usize,
    col: usize,
    visiting: &mut HashSet<(usize, usize)>,
) -> Result<SheetVal, &'static str> {
    let k = sheet_cell_key(row, col);
    let raw = cells.get(&k).map(|c| c.v.as_str()).unwrap_or("");
    let trimmed = raw.trim();
    if !trimmed.starts_with('=') || trimmed.len() == 1 {
        return Ok(sheet_literal_val(raw));
    }
    if !visiting.insert((row, col)) {
        return Err("#CICLO!");
    }
    let res = (|| -> Result<SheetVal, &'static str> {
        let toks = tokenize_sheet_formula(&trimmed[1..])?;
        let mut p = SheetFormulaParser {
            toks,
            pos: 0,
            cells,
            visiting,
        };
        let v = p.parse_cmp()?;
        let val = sheet_single(v)?;
        if let SheetVal::Num(n) = val {
            if !n.is_finite() {
                return Err("#DIV/0!");
            }
        }
        Ok(val)
    })();
    visiting.remove(&(row, col));
    res
}

pub fn eval_sheet_display(cells: &HashMap<String, SheetCell>, row: usize, col: usize) -> String {
    let k = sheet_cell_key(row, col);
    let raw = cells.get(&k).map(|c| c.v.as_str()).unwrap_or("");
    if !raw.trim_start().starts_with('=') || raw.trim().len() == 1 {
        return raw.to_string();
    }
    let mut visiting = HashSet::new();
    match eval_sheet_val_inner(cells, row, col, &mut visiting) {
        Ok(v) => sheet_to_text(v).unwrap_or_else(|e| e.to_string()),
        Err(e) => e.to_string(),
    }
}

fn sheet_used_size(cells: &HashMap<String, SheetCell>) -> (usize, usize) {
    let mut max_r = 0usize;
    let mut max_c = 0usize;
    for k in cells.keys() {
        if let Some((r, c)) = parse_sheet_coord(k) {
            max_r = max_r.max(r + 1);
            max_c = max_c.max(c + 1);
        }
    }
    (max_r, max_c)
}

/// Exporta a planilha como CSV separado por `;` com BOM UTF-8 (igual `sheetToCsv` do Web).
pub fn sheet_to_csv_semicolon(
    rows: usize,
    cols: usize,
    cells: &HashMap<String, SheetCell>,
) -> String {
    let (ur, uc) = sheet_used_size(cells);
    let r_lim = ur.min(rows);
    let c_lim = uc.min(cols);
    let mut lines = Vec::with_capacity(r_lim);
    for r in 0..r_lim {
        let mut fields = Vec::with_capacity(c_lim);
        for c in 0..c_lim {
            let v = eval_sheet_display(cells, r, c);
            if v.contains(';') || v.contains('"') || v.contains('\n') || v.contains('\r') {
                fields.push(format!("\"{}\"", v.replace('"', "\"\"")));
            } else {
                fields.push(v);
            }
        }
        lines.push(fields.join(";"));
    }
    format!("\u{FEFF}{}", lines.join("\r\n"))
}

fn esc_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Exporta `.xls` compativel com Microsoft Excel (`sheetToExcel` do Web).
pub fn sheet_to_excel_xls(
    rows: usize,
    cols: usize,
    cells: &HashMap<String, SheetCell>,
    col_widths: &HashMap<usize, u32>,
    title: &str,
) -> String {
    let (ur, uc) = sheet_used_size(cells);
    let r_lim = ur.min(rows);
    let c_lim = uc.min(cols);
    let mut col_tags = String::new();
    for c in 0..c_lim {
        let w = col_widths.get(&c).copied().unwrap_or(SHEET_DEFAULT_COL_WIDTH);
        col_tags.push_str(&format!("<col width=\"{}\">", w));
    }
    let mut body = String::new();
    for r in 0..r_lim {
        body.push_str("<tr>");
        for c in 0..c_lim {
            let k = sheet_cell_key(r, c);
            let cell = cells.get(&k);
            let disp = eval_sheet_display(cells, r, c);
            let mut styles = Vec::new();
            let mut extra = String::new();
            if let Some(cell) = cell {
                if cell.b {
                    styles.push("font-weight:bold".to_string());
                }
                if cell.i {
                    styles.push("font-style:italic".to_string());
                }
                if let Some(ref bg) = cell.bg {
                    styles.push(format!("background:{}", esc_xml(bg)));
                }
                if let Some(ref color) = cell.color {
                    styles.push(format!("color:{}", esc_xml(color)));
                }
                if let Some(ref align) = cell.align {
                    styles.push(format!("text-align:{}", esc_xml(align)));
                }
                if cell.v.starts_with('=') {
                    extra.push_str(&format!(" x:fmla=\"{}\"", esc_xml(&cell.v.replace(';', ","))));
                }
            }
            if let Ok(n) = disp.trim().replace(',', ".").parse::<f64>() {
                if n.is_finite() {
                    extra.push_str(&format!(" x:num=\"{}\"", n));
                }
            }
            let st_attr = if styles.is_empty() {
                String::new()
            } else {
                format!(" style=\"{}\"", styles.join(";"))
            };
            body.push_str(&format!("<td{}{}>{}</td>", st_attr, extra, esc_xml(&disp)));
        }
        body.push_str("</tr>");
    }
    let clean_title: String = if title.trim().is_empty() {
        "Planilha".to_string()
    } else {
        title.trim().chars().take(31).collect()
    };
    format!(
        "<html xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\" xmlns=\"http://www.w3.org/TR/REC-html40\"><head><meta charset=\"utf-8\"><!--[if gte mso 9]><xml><x:ExcelWorkbook><x:ExcelWorksheets><x:ExcelWorksheet><x:Name>{}</x:Name><x:WorksheetOptions><x:DisplayGridlines/></x:WorksheetOptions></x:ExcelWorksheet></x:ExcelWorksheets></x:ExcelWorkbook></xml><![endif]--></head><body><table>{}{}</table></body></html>",
        esc_xml(&clean_title),
        col_tags,
        body
    )
}

pub fn parse_pasted_tsv_grid(text: &str) -> Vec<Vec<String>> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = norm.strip_suffix('\n').unwrap_or(&norm);
    trimmed
        .split('\n')
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}

pub fn format_calc_number(val: f64) -> String {
    if !val.is_finite() {
        return "Erro".to_string();
    }
    let clean = if val.abs() < 1e-12 { 0.0 } else { val };
    let rounded = (clean * 1e10).round() / 1e10;
    let s = if rounded.fract() == 0.0 && rounded.abs() < 1e14 {
        format!("{:.0}", rounded)
    } else {
        let raw = format!("{:.10}", rounded);
        raw.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    s
}

pub fn press_calc_key(expr: &str, key: &str, fresh: bool, memory: f64) -> String {
    const OPS: &[&str] = &["+", "−", "×", "÷", "^"];
    const FNS: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "√"];
    let last_char = expr.chars().last().map(|c| c.to_string()).unwrap_or_default();
    let starts_over = fresh
        && !OPS.contains(&key)
        && key != "%"
        && key != "!"
        && key != "^2"
        && key != "1/x"
        && key != "±";
    let base = if starts_over { "" } else { expr };

    match key {
        "C" => String::new(),
        "⌫" => {
            if fresh {
                return String::new();
            }
            for f in ["asin(", "acos(", "atan(", "sin(", "cos(", "tan(", "ln(", "log(", "√(", "ans"] {
                if expr.ends_with(f) {
                    return expr[..expr.len() - f.len()].to_string();
                }
            }
            let mut chars: Vec<char> = expr.chars().collect();
            chars.pop();
            chars.into_iter().collect()
        }
        "±" => {
            if expr.is_empty() {
                return "−".to_string();
            }
            if let Some(rest) = expr.strip_prefix("−(").and_then(|s| s.strip_suffix(')')) {
                return rest.to_string();
            }
            if let Some(rest) = expr.strip_prefix('−') {
                if rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return rest.to_string();
                }
            } else if let Some(rest) = expr.strip_prefix('-') {
                if rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return rest.to_string();
                }
            } else if expr.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return format!("−{}", expr);
            }
            format!("−({})", expr)
        }
        "^2" => {
            if expr.is_empty() {
                String::new()
            } else {
                format!("{}^2", expr)
            }
        }
        "1/x" => {
            if expr.is_empty() {
                String::new()
            } else {
                format!("1÷({})", expr)
            }
        }
        "MR" => {
            let formatted = format_calc_number(memory).replacen('-', "−", 1);
            format!("{}{}", base, formatted)
        }
        "." | "," => {
            let tail = base
                .rsplit(['+', '−', '×', '÷', '^', '(', '-'])
                .next()
                .unwrap_or("");
            if tail.contains('.') || tail.contains(',') {
                base.to_string()
            } else if tail.is_empty() {
                format!("{}0.", base)
            } else {
                format!("{}.", base)
            }
        }
        _ if FNS.contains(&key) => format!("{}{}(", base, key),
        _ if OPS.contains(&key) => {
            if expr.is_empty() {
                return if key == "−" {
                    "−".to_string()
                } else {
                    String::new()
                };
            }
            if OPS.contains(&last_char.as_str()) && !(key == "−" && last_char != "−") {
                let mut chars: Vec<char> = expr.chars().collect();
                chars.pop();
                let prefix: String = chars.into_iter().collect();
                format!("{}{}", prefix, key)
            } else {
                format!("{}{}", expr, key)
            }
        }
        _ => format!("{}{}", base, key),
    }
}

pub fn eval_calc_preview(expr: &str, angle: &str) -> Option<String> {
    let cleaned = expr
        .trim()
        .replace(' ', "")
        .replace('÷', "/")
        .replace('×', "*")
        .replace('−', "-")
        .replace(',', ".");
    if cleaned.is_empty() {
        return None;
    }
    let mut p = CalcParser {
        chars: cleaned.chars().collect(),
        pos: 0,
        deg: angle != "rad",
    };
    let val = p.parse_expr().ok()?;
    if p.pos != p.chars.len() || !val.is_finite() {
        return None;
    }
    Some(format_calc_number(val))
}

struct CalcParser {
    chars: Vec<char>,
    pos: usize,
    deg: bool,
}

impl CalcParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn parse_expr(&mut self) -> Result<f64, ()> {
        let mut lhs = self.parse_term()?;
        while let Some(op) = self.peek() {
            if op == '+' || op == '-' {
                self.pos += 1;
                let rhs = self.parse_term()?;
                lhs = if op == '+' { lhs + rhs } else { lhs - rhs };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<f64, ()> {
        let mut lhs = self.parse_power()?;
        while let Some(op) = self.peek() {
            if op == '*' || op == '/' {
                self.pos += 1;
                let rhs = self.parse_power()?;
                if op == '/' {
                    if rhs.abs() < 1e-18 {
                        return Err(());
                    }
                    lhs /= rhs;
                } else {
                    lhs *= rhs;
                }
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_power(&mut self) -> Result<f64, ()> {
        let base = self.parse_unary()?;
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.parse_power()?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<f64, ()> {
        if self.peek() == Some('-') {
            self.pos += 1;
            return Ok(-self.parse_unary()?);
        }
        if self.peek() == Some('+') {
            self.pos += 1;
            return self.parse_unary();
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<f64, ()> {
        let mut v = self.parse_primary()?;
        while let Some(ch) = self.peek() {
            if ch == '%' {
                self.pos += 1;
                v /= 100.0;
            } else if ch == '!' {
                self.pos += 1;
                if v < 0.0 || v > 170.0 || v.fract().abs() > 1e-9 {
                    return Err(());
                }
                let n = v.round() as u64;
                let mut acc = 1.0_f64;
                for k in 2..=n {
                    acc *= k as f64;
                }
                v = acc;
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn parse_primary(&mut self) -> Result<f64, ()> {
        let Some(ch) = self.peek() else {
            return Err(());
        };
        if ch == '(' {
            self.pos += 1;
            let val = self.parse_expr()?;
            if self.peek() != Some(')') {
                return Err(());
            }
            self.pos += 1;
            return Ok(val);
        }
        if ch == 'π' {
            self.pos += 1;
            return Ok(std::f64::consts::PI);
        }
        if ch == '√' {
            self.pos += 1;
            let inner = if self.peek() == Some('(') {
                self.pos += 1;
                let v = self.parse_expr()?;
                if self.peek() != Some(')') {
                    return Err(());
                }
                self.pos += 1;
                v
            } else {
                self.parse_unary()?
            };
            if inner < 0.0 {
                return Err(());
            }
            return Ok(inner.sqrt());
        }
        if ch.is_ascii_alphabetic() {
            let start = self.pos;
            while self.pos < self.chars.len() && self.chars[self.pos].is_ascii_alphabetic() {
                self.pos += 1;
            }
            let ident: String = self.chars[start..self.pos].iter().collect();
            if ident.eq_ignore_ascii_case("pi") {
                return Ok(std::f64::consts::PI);
            }
            if ident.eq_ignore_ascii_case("e") {
                return Ok(std::f64::consts::E);
            }
            if self.peek() == Some('(') {
                self.pos += 1;
                let arg = self.parse_expr()?;
                if self.peek() != Some(')') {
                    return Err(());
                }
                self.pos += 1;
                let to_rad = |a: f64| if self.deg { a.to_radians() } else { a };
                let from_rad = |r: f64| if self.deg { r.to_degrees() } else { r };
                return match ident.to_ascii_lowercase().as_str() {
                    "sin" => Ok(to_rad(arg).sin()),
                    "cos" => Ok(to_rad(arg).cos()),
                    "tan" => Ok(to_rad(arg).tan()),
                    "asin" => Ok(from_rad(arg.asin())),
                    "acos" => Ok(from_rad(arg.acos())),
                    "atan" => Ok(from_rad(arg.atan())),
                    "ln" => Ok(arg.ln()),
                    "log" => Ok(arg.log10()),
                    "sqrt" => Ok(arg.sqrt()),
                    _ => Err(()),
                };
            }
            return Err(());
        }
        if ch.is_ascii_digit() || ch == '.' {
            let start = self.pos;
            while self.pos < self.chars.len()
                && (self.chars[self.pos].is_ascii_digit() || self.chars[self.pos] == '.')
            {
                self.pos += 1;
            }
            let num_str: String = self.chars[start..self.pos].iter().collect();
            return num_str.parse::<f64>().map_err(|_| ());
        }
        Err(())
    }
}

pub fn can_use_tool(tool: &RoomTool, self_user_id: Option<&str>, is_manager: bool) -> bool {
    if is_manager || tool.access == "everyone" {
        return true;
    }
    if tool.access == "selected" {
        return self_user_id.is_some_and(|uid| tool.allowed.iter().any(|a| a == uid));
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WbDragMode {
    Move,
    Resize,
}

#[derive(Debug, Clone)]
pub struct WbMoveDrag {
    pub tool_id: String,
    pub stroke_id: String,
    pub mode: WbDragMode,
    pub start_norm: [f32; 2],
    pub orig_points: Vec<f32>,
}

pub struct RoomToolsState {
    pub tools: Vec<RoomTool>,
    pub hands: Vec<RaisedHand>,
    pub speakers: Vec<String>,
    pub panel_open: bool,
    pub panel_expanded: bool,
    pub active_tool_id: Option<String>,
    pub creating_kind: Option<String>,

    // Estado local dos editores/desenho
    pub pen_color: String,
    pub pen_width: f32,
    pub pen_shape: String,
    pub active_draw_points: Vec<f32>,
    pub wb_live_strokes: HashMap<String, HashMap<String, ToolStroke>>,
    pub wb_selected_stroke: HashMap<String, String>,
    pub wb_move_drag: Option<WbMoveDrag>,
    pub wb_clipboard_error: Option<String>,
    pub text_syncs: HashMap<String, TextSyncClient>,
    pub calc_seq: HashMap<String, u64>,
    pub wheel_spin_starts: HashMap<String, Instant>,
    pub reactions: Vec<FloatingReaction>,
    reaction_seq: u64,
    stroke_seq: u64,

    // Rascunhos de UI
    pub create_title_draft: String,
    pub create_access_everyone: bool,
    pub create_code_lang: String,
    pub poll_question_draft: String,
    pub poll_options_draft: Vec<String>,
    pub poll_multi_draft: bool,
    pub task_input_draft: String,
    pub task_filter: HashMap<String, u8>,
    pub task_priority_draft: HashMap<String, String>,
    pub task_assignee_draft: HashMap<String, Option<String>>,
    pub task_due_draft: HashMap<String, String>,
    pub task_show_more: HashMap<String, bool>,
    pub task_show_rules: HashMap<String, bool>,
    pub task_editing_id: Option<(String, String)>,
    pub task_editing_draft: String,
    pub wheel_entry_draft: String,
    pub wheel_bulk_draft: String,
    pub wheel_tab: u8,
    pub wheel_num_from: String,
    pub wheel_num_to: String,
    pub wheel_bulk_mode: bool,
    pub wheel_create_num_open: bool,
    pub wheel_opt_num_open: HashMap<String, bool>,
    pub wheel_drawer_open: HashMap<String, bool>,
    pub wheel_winner_dismissed_spin_id: HashMap<String, String>,
    pub calc_scientific: bool,
    pub calc_fresh: bool,
    pub calc_history_drawer: HashMap<String, bool>,
    pub wb_text_draft: String,
    pub wb_pan_offset: [f32; 2],
    pub open_reaction_picker_tile: Option<String>,
    pub sheet_sel_row: usize,
    pub sheet_sel_col: usize,
    pub sheet_anchor_row: usize,
    pub sheet_anchor_col: usize,
    pub sheet_dragging_sel: bool,
    pub sheet_col_resize: Option<(String, usize, f32, f32)>,
    pub sheet_selected_cell: String,
    pub sheet_cell_draft: String,
    pub sheet_editing_inline: HashMap<String, bool>,
    pub sheet_inline_just_started: HashMap<String, bool>,
    pub sheet_color_popover: HashMap<String, bool>,
    pub sheet_palette_mode: HashMap<String, u8>,
    pub sheet_undo: Vec<(usize, usize, String)>,
    pub sheet_redo: Vec<(usize, usize, String)>,
    pub sheet_undo_blocks: Vec<(usize, usize, Vec<Vec<String>>, Vec<Vec<String>>)>,
    pub sheet_redo_blocks: Vec<(usize, usize, Vec<Vec<String>>, Vec<Vec<String>>)>,
    pub sheet_export_notice: HashMap<String, (String, Instant)>,
    pub doc_undo: HashMap<String, Vec<String>>,
    pub doc_redo: HashMap<String, Vec<String>>,
    pub doc_active_block: HashMap<String, usize>,
    pub doc_color_popover: HashMap<String, bool>,
    pub doc_link_popover: HashMap<String, bool>,
    pub doc_link_url_draft: HashMap<String, String>,
    pub doc_link_text_draft: HashMap<String, String>,
    pub doc_parsed_cache: HashMap<String, (u64, crate::net::doc_html::DocDocument)>,
    pub doc_pending_style: HashMap<String, crate::net::doc_html::InlineStyle>,
    pub doc_export_notice: HashMap<String, (String, Instant)>,
    pub chat_poll_open: bool,
    pub chat_poll_question: String,
    pub chat_poll_options: Vec<String>,
    pub chat_poll_multi: bool,
    pub chat_tasks_open: bool,
    pub hands_queue_collapsed: bool,
    /// Ultimo `error` do servidor sobre ferramentas (ex.: plano que nao inclui a ferramenta).
    pub server_error: Option<String>,
    /// "Anotar na tela": midias onde a pessoa desligou o lapis (o `annotateOff` do site).
    pub annotate_off: std::collections::HashSet<String>,
    /// Traco de anotacao em andamento: (chave da midia, pontos 0..1).
    pub annotate_drawing: Option<(String, Vec<f32>)>,
}

impl Default for RoomToolsState {
    fn default() -> Self {
        Self::new()
    }
}

impl RoomToolsState {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            hands: Vec::new(),
            speakers: Vec::new(),
            panel_open: false,
            panel_expanded: false,
            active_tool_id: None,
            creating_kind: None,
            pen_color: "#111827".to_string(),
            pen_width: 4.0,
            pen_shape: "pen".to_string(),
            active_draw_points: Vec::new(),
            wb_live_strokes: HashMap::new(),
            wb_selected_stroke: HashMap::new(),
            wb_move_drag: None,
            wb_clipboard_error: None,
            text_syncs: HashMap::new(),
            calc_seq: HashMap::new(),
            wheel_spin_starts: HashMap::new(),
            reactions: Vec::new(),
            reaction_seq: 0,
            stroke_seq: 0,
            create_title_draft: String::new(),
            create_access_everyone: true,
            create_code_lang: "javascript".to_string(),
            poll_question_draft: String::new(),
            poll_options_draft: vec![String::new(), String::new()],
            poll_multi_draft: false,
            task_input_draft: String::new(),
            task_filter: HashMap::new(),
            task_priority_draft: HashMap::new(),
            task_assignee_draft: HashMap::new(),
            task_due_draft: HashMap::new(),
            task_show_more: HashMap::new(),
            task_show_rules: HashMap::new(),
            task_editing_id: None,
            task_editing_draft: String::new(),
            wheel_entry_draft: String::new(),
            wheel_bulk_draft: "Opcao 1\nOpcao 2\nOpcao 3".to_string(),
            wheel_tab: 0,
            wheel_num_from: "1".to_string(),
            wheel_num_to: "10".to_string(),
            wheel_bulk_mode: false,
            wheel_create_num_open: false,
            wheel_opt_num_open: HashMap::new(),
            wheel_drawer_open: HashMap::new(),
            wheel_winner_dismissed_spin_id: HashMap::new(),
            calc_scientific: false,
            calc_fresh: false,
            calc_history_drawer: HashMap::new(),
            wb_text_draft: String::new(),
            wb_pan_offset: [0.0, 0.0],
            open_reaction_picker_tile: None,
            sheet_sel_row: 0,
            sheet_sel_col: 0,
            sheet_anchor_row: 0,
            sheet_anchor_col: 0,
            sheet_dragging_sel: false,
            sheet_col_resize: None,
            sheet_selected_cell: "0,0".to_string(),
            sheet_cell_draft: String::new(),
            sheet_editing_inline: HashMap::new(),
            sheet_inline_just_started: HashMap::new(),
            sheet_color_popover: HashMap::new(),
            sheet_palette_mode: HashMap::new(),
            sheet_undo: Vec::new(),
            sheet_redo: Vec::new(),
            sheet_undo_blocks: Vec::new(),
            sheet_redo_blocks: Vec::new(),
            sheet_export_notice: HashMap::new(),
            doc_undo: HashMap::new(),
            doc_redo: HashMap::new(),
            doc_active_block: HashMap::new(),
            doc_color_popover: HashMap::new(),
            doc_link_popover: HashMap::new(),
            doc_link_url_draft: HashMap::new(),
            doc_link_text_draft: HashMap::new(),
            doc_parsed_cache: HashMap::new(),
            doc_pending_style: HashMap::new(),
            doc_export_notice: HashMap::new(),
            chat_poll_open: false,
            chat_poll_question: String::new(),
            chat_poll_options: vec![String::new(), String::new()],
            chat_poll_multi: false,
            chat_tasks_open: false,
            hands_queue_collapsed: false,
            server_error: None,
            annotate_off: std::collections::HashSet::new(),
            annotate_drawing: None,
        }
    }

    /// Ao sair da sala: estado novo em folha (capacidade zero em todos os mapas/vetores, nada
    /// da sala anterior fica na RAM). So as preferencias de caneta e da calculadora ficam.
    pub fn reset(&mut self) {
        let fresh = Self {
            pen_color: std::mem::take(&mut self.pen_color),
            pen_width: self.pen_width,
            pen_shape: std::mem::take(&mut self.pen_shape),
            calc_scientific: self.calc_scientific,
            ..Self::new()
        };
        *self = fresh;
    }

    pub fn reactions_tool_id(&self) -> Option<&str> {
        self.tools
            .iter()
            .find(|t| t.kind == "reactions")
            .map(|t| t.id.as_str())
    }

    pub fn has_active_reactions(&self) -> bool {
        let now = Instant::now();
        self.reactions
            .iter()
            .any(|r| now.saturating_duration_since(r.created_at).as_secs_f32() < 3.45)
    }

    pub fn prune_expired_reactions(&mut self) {
        let now = Instant::now();
        self.reactions
            .retain(|r| now.saturating_duration_since(r.created_at).as_secs_f32() < 3.5);
    }

    pub fn has_active_wheel_spin(&self) -> bool {
        self.tools.iter().any(|t| {
            if let ToolPayload::Wheel { spin: Some(ref sp), .. } = t.payload {
                sp.local_start.elapsed().as_secs_f32() < sp.duration_secs
            } else {
                false
            }
        })
    }

    pub fn next_stroke_client_id(&mut self) -> String {
        self.stroke_seq = self.stroke_seq.wrapping_add(1);
        format!("rs-st-{}", self.stroke_seq)
    }

    pub fn edit_text_tool(&mut self, tool_id: &str, next: &str) -> Option<Value> {
        let sync = self.text_syncs.get_mut(tool_id)?;
        let msg = sync.edit(tool_id, next);
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_id) {
            if let ToolPayload::Text { ref mut text, .. } = tool.payload {
                *text = sync.text.clone();
            }
        }
        msg
    }

    /// Aplica alteracao idempotente de planilha localmente na hora (como `editSheet` no Web).
    pub fn apply_sheet_change_local(&mut self, tool_id: &str, change: &Value) {
        let op = change
            .get("op")
            .or_else(|| change.get("type"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if !matches!(op, "set" | "cell" | "fill" | "style" | "width") {
            return;
        }
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_id) {
            if let ToolPayload::Sheet {
                ref mut rows,
                ref mut cols,
                ref mut cells,
                ref mut col_widths,
            } = tool.payload
            {
                apply_sheet_change(rows, cols, cells, col_widths, change);
            }
        }
    }

    pub fn handle_server_message(
        &mut self,
        msg: &Value,
        self_id: Option<&str>,
    ) -> Vec<Value> {
        let mut follow_ups = Vec::new();
        let kind = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "room-tools" => {
                self.tools.clear();
                self.text_syncs.clear();
                self.calc_seq.clear();
                self.wheel_spin_starts.clear();
                self.wb_live_strokes.clear();
                self.doc_parsed_cache.clear();
                if let Some(arr) = msg.get("tools").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let Some(tool) =
                            RoomTool::from_json_with_starts(item, Some(&mut self.wheel_spin_starts))
                        {
                            if let ToolPayload::Text {
                                ref text, version, ..
                            } = tool.payload
                            {
                                self.text_syncs.insert(
                                    tool.id.clone(),
                                    TextSyncClient::new(text.clone(), version),
                                );
                            }
                            self.tools.push(tool);
                        }
                    }
                }
                self.hands = msg
                    .get("hands")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                self.speakers = msg
                    .get("speakers")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                if let Some(ref aid) = self.active_tool_id {
                    if !self.tools.iter().any(|t| &t.id == aid) {
                        self.active_tool_id = None;
                    }
                }
            }
            "tool-upsert" => {
                if let Some(raw_tool) = msg.get("tool") {
                    if let Some(mut tool) = RoomTool::from_json_with_starts(
                        raw_tool,
                        Some(&mut self.wheel_spin_starts),
                    ) {
                        let is_new = !self.tools.iter().any(|t| t.id == tool.id);
                        if let ToolPayload::Text {
                            ref mut text,
                            ref mut version,
                            ..
                        } = tool.payload
                        {
                            let force_reset = msg
                                .get("rejectedOpId")
                                .or_else(|| msg.get("resync"))
                                .is_some();
                            match self.text_syncs.get_mut(&tool.id) {
                                None => {
                                    self.text_syncs.insert(
                                        tool.id.clone(),
                                        TextSyncClient::new(text.clone(), *version),
                                    );
                                }
                                Some(sync) if force_reset || !sync.dirty() => {
                                    sync.reset(text.clone(), *version);
                                }
                                Some(sync) => {
                                    *text = sync.text.clone();
                                    *version = sync.version;
                                }
                            }
                        }
                        if is_new
                            && self.creating_kind.as_deref() == Some(tool.kind.as_str())
                        {
                            self.creating_kind = None;
                            if is_media_tool_kind(&tool.kind) {
                                self.panel_open = false;
                                self.active_tool_id = None;
                            } else if tool.kind != "poll" && tool.kind != "reactions" {
                                self.active_tool_id = Some(tool.id.clone());
                            }
                        }
                        if let Some(slot) = self.tools.iter_mut().find(|t| t.id == tool.id) {
                            *slot = tool;
                        } else {
                            self.tools.push(tool);
                        }
                    }
                }
            }
            "tool-removed" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    self.tools.retain(|t| t.id != id);
                    self.text_syncs.remove(id);
                    self.calc_seq.remove(id);
                    self.wb_live_strokes.remove(id);
                    self.wb_selected_stroke.remove(id);
                    self.doc_parsed_cache.remove(id);
                    self.doc_undo.remove(id);
                    self.doc_redo.remove(id);
                    self.doc_active_block.remove(id);
                    self.wheel_drawer_open.remove(id);
                    self.calc_history_drawer.remove(id);
                    self.sheet_editing_inline.remove(id);
                    self.task_filter.remove(id);
                    if self.active_tool_id.as_deref() == Some(id) {
                        self.active_tool_id = None;
                    }
                }
            }
            "tool-meta" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                        if let Some(t) = msg.get("title").and_then(|v| v.as_str()) {
                            tool.title = t.to_string();
                        }
                        if let Some(acc) = msg.get("access").and_then(|v| v.as_str()) {
                            tool.access = acc.to_string();
                        }
                        if let Some(lang) = msg.get("language").and_then(|v| v.as_str()) {
                            if let ToolPayload::Text {
                                ref mut language, ..
                            } = tool.payload
                            {
                                *language = lang.to_string();
                            }
                        }
                    }
                }
            }
            "tool-stroke" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    if let Some(client_id) = msg.get("clientId").and_then(|v| v.as_str()) {
                        let by = msg
                            .get("stroke")
                            .and_then(|s| s.get("by"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Some(live_map) = self.wb_live_strokes.get_mut(id) {
                            live_map.remove(&format!("{}:{}", by, client_id));
                        }
                    }
                    if let Some(Ok(stroke)) = msg
                        .get("stroke")
                        .map(|s| serde_json::from_value::<ToolStroke>(s.clone()))
                    {
                        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                            match &mut tool.payload {
                                ToolPayload::Whiteboard { ref mut strokes }
                                | ToolPayload::Annotate { ref mut strokes } => {
                                    if !strokes.iter().any(|s| s.id == stroke.id) {
                                        strokes.push(stroke);
                                        if strokes.len() > 3000 {
                                            let excess = strokes.len() - 3000;
                                            strokes.drain(0..excess);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "tool-stroke-live" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    let by = msg.get("by").and_then(|v| v.as_str()).unwrap_or("");
                    let live_id = msg.get("liveId").and_then(|v| v.as_str()).unwrap_or("");
                    if !by.is_empty() && !live_id.is_empty() {
                        let key = format!("{}:{}", by, live_id);
                        if msg.get("end").and_then(|v| v.as_bool()).unwrap_or(false) {
                            if let Some(map) = self.wb_live_strokes.get_mut(id) {
                                map.remove(&key);
                            }
                        } else {
                            let pts: Vec<f32> = msg
                                .get("points")
                                .and_then(|v| v.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let offset = msg.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let shape = msg
                                .get("shape")
                                .and_then(|v| v.as_str())
                                .unwrap_or("pen")
                                .to_string();
                            let color = msg
                                .get("color")
                                .and_then(|v| v.as_str())
                                .unwrap_or("#111827")
                                .to_string();
                            let width = msg.get("width").and_then(|v| v.as_f64()).unwrap_or(4.0) as f32;
                            let entry_map = self.wb_live_strokes.entry(id.to_string()).or_default();
                            if offset == 0 {
                                entry_map.insert(
                                    key,
                                    ToolStroke {
                                        id: live_id.to_string(),
                                        by: by.to_string(),
                                        shape,
                                        color,
                                        width,
                                        points: pts,
                                        text: None,
                                        src: None,
                                        target: msg
                                            .get("target")
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string),
                                    },
                                );
                            } else if let Some(prev) = entry_map.get_mut(&key) {
                                if prev.points.len() == offset {
                                    prev.points.extend(pts);
                                }
                            }
                        }
                    }
                }
            }
            "tool-stroke-moved" => {
                if let (Some(id), Some(stroke_id), Some(pts_arr)) = (
                    msg.get("id").and_then(|v| v.as_str()),
                    msg.get("strokeId").and_then(|v| v.as_str()),
                    msg.get("points").and_then(|v| v.as_array()),
                ) {
                    let new_pts: Vec<f32> = pts_arr
                        .iter()
                        .filter_map(|x| x.as_f64().map(|f| f as f32))
                        .collect();
                    if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                        match &mut tool.payload {
                            ToolPayload::Whiteboard { ref mut strokes }
                            | ToolPayload::Annotate { ref mut strokes } => {
                                if let Some(st) = strokes.iter_mut().find(|s| s.id == stroke_id) {
                                    st.points = new_pts;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "tool-strokes-removed" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    let remove_ids: HashSet<String> = msg
                        .get("strokeIds")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    let clear_all = msg.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
                    if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                        match &mut tool.payload {
                            ToolPayload::Whiteboard { ref mut strokes }
                            | ToolPayload::Annotate { ref mut strokes } => {
                                if clear_all {
                                    strokes.clear();
                                } else {
                                    strokes.retain(|s| !remove_ids.contains(&s.id));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "tool-text-op" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    let op: Vec<TextPrim> = msg
                        .get("op")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    let version = msg.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
                    let op_id = msg.get("opId").and_then(|v| v.as_str());
                    if let Some(sync) = self.text_syncs.get_mut(id) {
                        let (out_of_step, next_cmd) = sync.receive(id, &op, version, op_id);
                        if out_of_step {
                            follow_ups.push(json!({ "type": "tool-sync", "id": id }));
                        } else {
                            if let Some(cmd) = next_cmd {
                                follow_ups.push(cmd);
                            }
                            if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                                if let ToolPayload::Text {
                                    ref mut text,
                                    version: ref mut v,
                                    ..
                                } = tool.payload
                                {
                                    *text = sync.text.clone();
                                    *v = sync.version;
                                }
                            }
                        }
                    }
                }
            }
            "tool-sheet" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    if let Some(change) = msg.get("change") {
                        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                            if let ToolPayload::Sheet {
                                ref mut rows,
                                ref mut cols,
                                ref mut cells,
                                ref mut col_widths,
                            } = tool.payload
                            {
                                apply_sheet_change(rows, cols, cells, col_widths, change);
                            }
                        }
                    }
                }
            }
            "tool-reaction" => {
                let emoji = msg
                    .get("emoji")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .trim();
                if !emoji.is_empty() {
                    let target = msg
                        .get("target")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = msg
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.prune_expired_reactions();
                    self.reaction_seq = self.reaction_seq.wrapping_add(1);
                    let x_frac = 0.14 + (((self.reaction_seq * 37) % 72) as f32) / 100.0;
                    self.reactions.push(FloatingReaction {
                        id: format!("rx-{}", self.reaction_seq),
                        target,
                        emoji: emoji.to_string(),
                        name,
                        x_frac,
                        created_at: Instant::now(),
                    });
                    if self.reactions.len() > 32 {
                        let excess = self.reactions.len() - 32;
                        self.reactions.drain(0..excess);
                    }
                }
            }
            "tool-calc-expr" => {
                if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                    if let Some(expr) = msg.get("expr").and_then(|s| s.as_str()) {
                        let from_self = msg.get("from").and_then(|s| s.as_str()) == self_id;
                        let seq = msg.get("seq").and_then(|n| n.as_u64());
                        let stale_echo = from_self && seq != self.calc_seq.get(id).copied();
                        if !stale_echo {
                            if let Some(tool) = self.tools.iter_mut().find(|t| t.id == id) {
                                if let ToolPayload::Calc { expr: ref mut ex, .. } = tool.payload {
                                    *ex = expr.to_string();
                                }
                            }
                        }
                    }
                }
            }
            "room-hands" => {
                self.hands = msg
                    .get("hands")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                self.speakers = msg
                    .get("speakers")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
            }
            _ => {}
        }
        follow_ups
    }
}

pub fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if clean.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let mut i = 0usize;
    while i < clean.len() {
        let a = val(*clean.get(i)?)?;
        let b = val(*clean.get(i + 1)?)?;
        let c_b = clean.get(i + 2).copied().unwrap_or(b'=');
        let d_b = clean.get(i + 3).copied().unwrap_or(b'=');
        out.push((a << 2) | (b >> 4));
        if c_b != b'=' {
            let c = val(c_b)?;
            out.push((b << 4) | (c >> 2));
            if d_b != b'=' {
                let d = val(d_b)?;
                out.push((c << 6) | d);
            }
        }
        i += 4;
    }
    Some(out)
}

pub fn encode_base64(bytes: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(TBL[b0 >> 2] as char);
        out.push(TBL[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(TBL[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TBL[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn decode_data_url_image(src: &str) -> Option<Vec<u8>> {
    let trimmed = src.trim();
    if !trimmed.to_ascii_lowercase().starts_with("data:image/") {
        return None;
    }
    let (_, b64) = trimmed.split_once(";base64,")?;
    decode_base64(b64)
}

pub fn encode_png_data_url(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let (sw, sh, small) = crate::media::image_decode::downscale_rgba_if_needed(
        width,
        height,
        rgba.to_vec(),
        520,
    );
    let png = crate::media::image_decode::encode_rgba_to_png(sw, sh, &small).ok()?;
    if png.len() > 750 * 1024 {
        return None;
    }
    Some(format!("data:image/png;base64,{}", encode_base64(&png)))
}

pub fn cmd_create_tool(kind: &str, options: Value) -> Value {
    let mut map = match options {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    map.insert("type".to_string(), Value::String("tool-create".to_string()));
    map.insert("kind".to_string(), Value::String(kind.to_string()));
    Value::Object(map)
}

pub fn cmd_reaction(tool_id: &str, emoji: &str, target: &str) -> Value {
    json!({
        "type": "tool-reaction",
        "id": tool_id,
        "emoji": emoji,
        "target": target,
    })
}

pub fn cmd_close_tool(id: &str) -> Value {
    json!({ "type": "tool-close", "id": id })
}

pub fn cmd_add_stroke(
    tool_id: &str,
    shape: &str,
    color: &str,
    width: f32,
    points: &[f32],
    client_id: &str,
) -> Value {
    cmd_add_stroke_on(tool_id, shape, color, width, points, client_id, None)
}

/// Um traco com `target` (a midia, para "Anotar na tela").
pub fn cmd_add_stroke_on(
    tool_id: &str,
    shape: &str,
    color: &str,
    width: f32,
    points: &[f32],
    client_id: &str,
    target: Option<&str>,
) -> Value {
    let mut stroke = json!({
        "shape": shape,
        "color": color,
        "width": width,
        "points": points,
    });
    if let Some(t) = target {
        stroke["target"] = Value::String(t.to_string());
    }
    json!({
        "type": "tool-stroke",
        "id": tool_id,
        "clientId": client_id,
        "stroke": stroke,
    })
}

pub fn cmd_image_stroke(
    tool_id: &str,
    points: [f32; 4],
    data_url: &str,
    client_id: &str,
) -> Value {
    json!({
        "type": "tool-stroke",
        "id": tool_id,
        "clientId": client_id,
        "stroke": {
            "shape": "image",
            "color": "#000000",
            "width": 1,
            "points": points,
            "src": data_url,
        }
    })
}

pub fn cmd_move_stroke(tool_id: &str, stroke_id: &str, points: &[f32]) -> Value {
    json!({
        "type": "tool-stroke-move",
        "id": tool_id,
        "strokeId": stroke_id,
        "points": points,
    })
}

pub fn cmd_remove_strokes(tool_id: &str, ids: &[String]) -> Value {
    json!({ "type": "tool-strokes-remove", "id": tool_id, "strokeIds": ids })
}

pub fn cmd_clear_strokes(tool_id: &str) -> Value {
    json!({ "type": "tool-strokes-remove", "id": tool_id, "all": true })
}

pub fn cmd_code_language(tool_id: &str, language: &str) -> Value {
    json!({ "type": "tool-code-language", "id": tool_id, "language": language })
}

pub fn cmd_poll_vote(tool_id: &str, option_ids: &[String]) -> Value {
    json!({ "type": "tool-poll-vote", "id": tool_id, "optionIds": option_ids })
}

pub fn cmd_poll_close(tool_id: &str) -> Value {
    json!({ "type": "tool-poll-close", "id": tool_id })
}

pub fn cmd_task_add_full(
    tool_id: &str,
    text: &str,
    assignee_id: Option<&str>,
    priority: &str,
    due: Option<&str>,
) -> Value {
    json!({
        "type": "tool-task",
        "id": tool_id,
        "action": "add",
        "text": text,
        "assigneeId": assignee_id,
        "priority": priority,
        "due": due,
    })
}

pub fn cmd_task_toggle(tool_id: &str, item_id: &str) -> Value {
    json!({ "type": "tool-task", "id": tool_id, "action": "toggle", "itemId": item_id })
}

pub fn cmd_task_edit(tool_id: &str, item_id: &str, text: &str, priority: Option<&str>) -> Value {
    let mut obj = json!({
        "type": "tool-task",
        "id": tool_id,
        "action": "edit",
        "itemId": item_id,
        "text": text,
    });
    if let Some(p) = priority {
        obj["priority"] = Value::String(p.to_string());
    }
    obj
}

/// `roomTools.editTask(..., { assigneeId })` do site (`None` = sem responsavel).
pub fn cmd_task_set_assignee(tool_id: &str, item_id: &str, assignee_id: Option<&str>) -> Value {
    json!({
        "type": "tool-task",
        "id": tool_id,
        "action": "edit",
        "itemId": item_id,
        "assigneeId": assignee_id,
    })
}

/// `roomTools.editTask(..., { priority })` do site.
pub fn cmd_task_set_priority(tool_id: &str, item_id: &str, priority: &str) -> Value {
    json!({
        "type": "tool-task",
        "id": tool_id,
        "action": "edit",
        "itemId": item_id,
        "priority": priority,
    })
}

/// Data local de hoje como `AAAA-MM-DD` (o mesmo formato do `<input type="date">` do site).
pub fn today_iso_local() -> String {
    let (y, m, d) = local_ymd();
    format!("{y:04}-{m:02}-{d:02}")
}

/// Soma `days` dias a uma data `AAAA-MM-DD` valida.
pub fn add_days_iso(iso: &str, days: i64) -> Option<String> {
    let (y, m, d) = parse_iso_date(iso)?;
    let (y, m, d) = civil_from_days(days_from_civil(y, m, d) + days);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Valida `AAAA-MM-DD` (ano 2000..2100, dia existente no mes).
pub fn parse_iso_date(s: &str) -> Option<(i64, u32, u32)> {
    let s = s.trim();
    if s.len() != 10 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: u32 = s[5..7].parse().ok()?;
    let d: u32 = s[8..10].parse().ok()?;
    if !(2000..=2100).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max_d = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (d <= max_d).then_some((y, m, d))
}

/// `formatDue` do site em pt-BR: `05 de dez.` -> aqui `05 dez`.
pub fn format_due_short(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
    ];
    match parse_iso_date(iso) {
        Some((_, m, d)) => format!("{d:02} {}", MONTHS[(m - 1) as usize]),
        None => iso.to_string(),
    }
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(windows)]
fn local_ymd() -> (i64, u32, u32) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(st: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    // SAFETY: GetLocalTime so escreve na struct SYSTEMTIME que passamos.
    unsafe { GetLocalTime(&mut st) };
    (st.year as i64, st.month as u32, st.day as u32)
}

#[cfg(not(windows))]
fn local_ymd() -> (i64, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    civil_from_days(secs / 86_400)
}

pub fn cmd_task_remove(tool_id: &str, item_id: &str) -> Value {
    json!({ "type": "tool-task", "id": tool_id, "action": "remove", "itemId": item_id })
}

pub fn cmd_task_clear_done(tool_id: &str) -> Value {
    json!({ "type": "tool-task", "id": tool_id, "action": "clear-done" })
}

pub fn cmd_task_set_rules(tool_id: &str, rules: &TaskRules) -> Value {
    json!({ "type": "tool-task", "id": tool_id, "action": "rules", "rules": rules })
}

pub fn cmd_hand_raise(raised: bool) -> Value {
    json!({ "type": "hand-raise", "raised": raised })
}

pub fn cmd_hand_action(action_type: &str, user_id: &str) -> Value {
    json!({ "type": action_type, "userId": user_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16_text_ot_diff_and_apply_with_accents() {
        let before = "Olá sala!";
        let after = "Olá minha sala incrível!";
        let op = diff_text_utf16(before, after);
        assert_eq!(apply_text_op(before, &op), after);

        let before2 = "Ação rápida 🎯";
        let after2 = "Ação concluída";
        let op2 = diff_text_utf16(before2, after2);
        assert_eq!(apply_text_op(before2, &op2), after2);
    }

    #[test]
    fn test_room_tools_state_handles_snapshot_upsert_stroke_move_and_remove() {
        let mut st = RoomToolsState::new();
        let snap = json!({
            "type": "room-tools",
            "tools": [{
                "id": "wb1",
                "kind": "whiteboard",
                "title": "Lousa",
                "createdById": "u1",
                "createdByName": "Joao",
                "access": "everyone",
                "allowed": [],
                "strokes": []
            }],
            "hands": [{ "id": "u2", "name": "Maria", "at": 100 }],
            "speakers": []
        });
        assert!(st.handle_server_message(&snap, Some("p1")).is_empty());
        assert_eq!(st.tools.len(), 1);
        assert_eq!(st.hands.len(), 1);

        let stroke_msg = json!({
            "type": "tool-stroke",
            "id": "wb1",
            "stroke": {
                "id": "st1",
                "by": "u1",
                "shape": "rect",
                "color": "#ef4444",
                "width": 4.0,
                "points": [0.1, 0.2, 0.4, 0.5]
            }
        });
        st.handle_server_message(&stroke_msg, Some("p1"));
        match &st.tools[0].payload {
            ToolPayload::Whiteboard { strokes } => assert_eq!(strokes.len(), 1),
            _ => panic!("expected whiteboard"),
        }

        let moved_msg = json!({
            "type": "tool-stroke-moved",
            "id": "wb1",
            "strokeId": "st1",
            "points": [0.2, 0.3, 0.5, 0.6]
        });
        st.handle_server_message(&moved_msg, Some("p1"));
        match &st.tools[0].payload {
            ToolPayload::Whiteboard { strokes } => {
                assert_eq!(strokes[0].points, vec![0.2, 0.3, 0.5, 0.6]);
            }
            _ => panic!("expected whiteboard"),
        }

        let rm = json!({ "type": "tool-removed", "id": "wb1" });
        st.handle_server_message(&rm, Some("p1"));
        assert!(st.tools.is_empty());
    }

    #[test]
    fn test_tasks_null_safe_deserialization_and_rules() {
        let raw = json!({
            "id": "tk1",
            "kind": "tasks",
            "title": "Tarefas",
            "items": [{
                "id": "i1",
                "text": "Testar build",
                "done": false,
                "byName": null,
                "doneByName": null,
                "assigneeId": "u2",
                "assigneeName": null,
                "priority": null,
                "due": null,
                "createdAt": null
            }],
            "rules": {
                "add": "everyone",
                "complete": "assignee",
                "edit": "managers"
            }
        });
        let tool = RoomTool::from_json(&raw).expect("tasks tool must parse even with null fields");
        match tool.payload {
            ToolPayload::Tasks { items, rules } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].priority, "normal");
                assert!(task_allows(&rules, "add", Some("u9"), false, None));
                assert!(task_allows(&rules, "complete", Some("u2"), false, Some(&items[0])));
                assert!(!task_allows(&rules, "complete", Some("u9"), false, Some(&items[0])));
                assert!(task_allows(&rules, "edit", Some("u9"), true, Some(&items[0])));
                assert!(!task_allows(&rules, "edit", Some("u9"), false, Some(&items[0])));
            }
            _ => panic!("expected tasks"),
        }
    }

    #[test]
    fn test_base64_and_data_url_roundtrip() {
        let raw = b"hello world\x00\xff";
        let b64 = encode_base64(raw);
        assert_eq!(decode_base64(&b64).as_deref(), Some(&raw[..]));
        let du = format!("data:image/png;base64,{}", b64);
        assert_eq!(decode_data_url_image(&du).as_deref(), Some(&raw[..]));
    }

    #[test]
    fn test_calc_eval_and_press_key() {
        assert_eq!(eval_calc_preview("54÷5", "deg").as_deref(), Some("10.8"));
        assert_eq!(eval_calc_preview("sin(30)+2", "deg").as_deref(), Some("2.5"));
        assert_eq!(press_calc_key("12+", "−", false, 0.0), "12+−");
        assert_eq!(press_calc_key("99", "5", true, 0.0), "5");
    }

    #[test]
    fn test_wheel_spin_ms_and_preserve_start() {
        let mut st = RoomToolsState::new();
        let upsert1 = json!({
            "type": "tool-upsert",
            "tool": {
                "id": "w1",
                "kind": "wheel",
                "title": "Roleta",
                "entries": [{"id": "e1", "text": "A", "color": "#10b981", "weight": 1}],
                "spin": {
                    "id": "spin-123",
                    "winnerText": "A",
                    "byName": "Lucas",
                    "duration": 5000,
                    "from": 0.0,
                    "to": 1440.0,
                    "elapsed": 500,
                    "entries": []
                }
            }
        });
        st.handle_server_message(&upsert1, Some("p1"));
        let start1 = match &st.tools[0].payload {
            ToolPayload::Wheel { spin: Some(sp), .. } => {
                assert!((sp.duration_secs - 5.0).abs() < 0.01);
                sp.local_start
            }
            _ => panic!("expected wheel spin"),
        };

        std::thread::sleep(std::time::Duration::from_millis(20));
        st.handle_server_message(&upsert1, Some("p1"));
        let start2 = match &st.tools[0].payload {
            ToolPayload::Wheel { spin: Some(sp), .. } => sp.local_start,
            _ => panic!("expected wheel spin"),
        };
        assert_eq!(start1, start2);
    }

    #[test]
    fn test_sheet_ops_formulas_and_reactions() {
        let mut rows = 6usize;
        let mut cols = 4usize;
        let mut cells = std::collections::HashMap::new();
        let mut col_widths = std::collections::HashMap::new();
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "0,0", "v": "10" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "1,0", "v": "15" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "2,0", "v": "=SOMA(A1:A2)" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "3,0", "v": "=SE(A3>20; \"Alto\"; \"Baixo\")" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "4,0", "v": "=1/0" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "style", "cells": ["0,0"], "style": { "b": true, "bg": "#fef08a" } }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "width", "col": 1, "width": 160 }));
        assert_eq!(eval_sheet_display(&cells, 2, 0), "25");
        assert_eq!(eval_sheet_display(&cells, 3, 0), "Alto");
        assert_eq!(eval_sheet_display(&cells, 4, 0), "#DIV/0!");
        assert!(cells.get("0,0").unwrap().b);
        assert_eq!(col_widths.get(&1), Some(&160));

        // Ciclo A6 -> B6 -> A6
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "5,0", "v": "=B6" }));
        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "set", "cell": "5,1", "v": "=A6" }));
        assert_eq!(eval_sheet_display(&cells, 5, 0), "#CICLO!");

        apply_sheet_change(&mut rows, &mut cols, &mut cells, &mut col_widths, &json!({ "op": "insert", "axis": "col", "at": 1 }));
        assert_eq!(cols, 5);
        assert_eq!(col_widths.get(&2), Some(&160));

        let csv = sheet_to_csv_semicolon(rows, cols, &cells);
        assert!(csv.starts_with('\u{FEFF}'));
        assert!(csv.contains("25;"));
        let xls = sheet_to_excel_xls(rows, cols, &cells, &col_widths, "Vendas");
        assert!(xls.contains("urn:schemas-microsoft-com:office:excel"));
        assert!(xls.contains("font-weight:bold"));
        assert!(xls.contains("<col width=\"160\">"));
        assert_eq!(parse_pasted_tsv_grid("a\tb\r\nc\td\n"), vec![vec!["a", "b"], vec!["c", "d"]]);

        let mut st = RoomToolsState::new();
        st.handle_server_message(
            &json!({ "type": "tool-reaction", "emoji": "🔥", "name": "Lucas", "target": "screen:p1" }),
            Some("p1"),
        );
        assert!(st.has_active_reactions());
        assert_eq!(st.reactions.len(), 1);
    }
}
