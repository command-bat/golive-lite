use egui::{Color32, Rounding, Stroke, Vec2, Visuals};

// Paleta da sala nova do site (experiment room-redesign, app/globals.css [data-room-redesign]):
// pagina #0d1117 < cartao zinc-950 #161b22 < controle zinc-900 #1c222b < borda zinc-800 #2a313c.
pub const BG_MAIN: Color32 = Color32::from_rgb(13, 17, 23); // #0d1117 (pagina da sala)
pub const BG_SURFACE: Color32 = Color32::from_rgb(22, 27, 34); // #161b22 (zinc-950: cartoes)
pub const BG_ELEVATED: Color32 = Color32::from_rgb(28, 34, 43); // #1c222b (zinc-900)
pub const BG_TILE: Color32 = Color32::from_rgb(22, 27, 34); // #161b22 (tile = cartao)
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(42, 49, 60); // #2a313c (zinc-800)
pub const BORDER_STRONG: Color32 = Color32::from_rgb(58, 66, 78); // #3a424e (zinc-700)

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(233, 233, 236); // #e9e9ec
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(158, 160, 172); // zinc-400 azulado
pub const TEXT_MUTED: Color32 = Color32::from_rgb(110, 113, 124); // zinc-500 azulado

pub const EMERALD: Color32 = Color32::from_rgb(5, 150, 105); // #059669
pub const EMERALD_HOVER: Color32 = Color32::from_rgb(16, 185, 129); // #10b981
pub const EMERALD_BRIGHT: Color32 = Color32::from_rgb(34, 197, 94); // #22c55e
pub const BLUE: Color32 = Color32::from_rgb(37, 99, 235); // #2563eb
pub const BLUE_LIGHT: Color32 = Color32::from_rgb(96, 165, 250); // #60a5fa
pub const RED: Color32 = Color32::from_rgb(220, 38, 38); // #dc2626
pub const AMBER: Color32 = Color32::from_rgb(245, 158, 11); // #f59e0b
pub const GOLD: Color32 = Color32::from_rgb(251, 191, 36); // #fbbf24
pub const RUBY: Color32 = Color32::from_rgb(244, 63, 94); // #f43f5e
pub const BLUE_SEAL: Color32 = Color32::from_rgb(59, 130, 246); // #3b82f6
pub const INDIGO: Color32 = Color32::from_rgb(129, 140, 248); // #818cf8
pub const CTA_BLUE: Color32 = Color32::from_rgb(79, 117, 255); // #4f75ff
pub const DISCORD_BLURPLE: Color32 = Color32::from_rgb(88, 101, 242); // #5865f2

/// Familia da fonte semibold (Plus Jakarta Sans 600), para o `font-semibold` do site.
pub const SEMIBOLD: &str = "semibold";

/// `RichText::semibold()`: o `font-semibold` do site (a fonte de verdade, nao so a cor).
pub trait SemiBoldExt {
    fn semibold(self) -> egui::RichText;
}

impl SemiBoldExt for egui::RichText {
    fn semibold(self) -> egui::RichText {
        self.strong()
            .family(egui::FontFamily::Name(SEMIBOLD.into()))
    }
}

/// Fonte do site na sala nova: Plus Jakarta Sans (`--font-jakarta`), 400 e 600. As fontes
/// embutidas do egui ficam depois, so para simbolos/emoji que a Jakarta nao tem.
/// Custo: ~260 KB no executavel; o atlas so guarda os glifos que aparecem na tela.
fn install_golive_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "jakarta".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/PlusJakartaSans-Regular.ttf")),
    );
    fonts.font_data.insert(
        "jakarta-semibold".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/PlusJakartaSans-SemiBold.ttf")),
    );
    let fallback = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut regular = vec!["jakarta".to_string()];
    regular.extend(fallback.iter().cloned());
    let mut semibold = vec!["jakarta-semibold".to_string()];
    semibold.extend(fallback.iter().cloned());
    fonts.families.insert(egui::FontFamily::Proportional, regular);
    fonts
        .families
        .insert(egui::FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}

pub fn apply_golive_theme(ctx: &egui::Context) {
    install_golive_fonts(ctx);
    let mut style = (*ctx.style()).clone();
    let mut visuals = Visuals::dark();

    visuals.window_fill = BG_SURFACE;
    visuals.panel_fill = BG_MAIN;
    visuals.faint_bg_color = BG_SURFACE;
    visuals.extreme_bg_color = BG_TILE;
    visuals.code_bg_color = BG_ELEVATED;

    visuals.widgets.noninteractive.bg_fill = BG_SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.rounding = Rounding::same(8.0);

    visuals.widgets.inactive.bg_fill = BG_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.rounding = Rounding::same(8.0);

    visuals.widgets.hovered.bg_fill = BORDER_SUBTLE; // hover:bg-zinc-800
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.rounding = Rounding::same(8.0);

    visuals.widgets.active.bg_fill = BORDER_STRONG; // zinc-700
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, EMERALD);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.rounding = Rounding::same(8.0);

    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(5, 150, 105, 85);
    visuals.selection.stroke = Stroke::new(1.0_f32, EMERALD_HOVER);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    ctx.set_style(style);
}

/// Cria uma `egui::Window` inicialmente centralizada no app e livre para arrastar dentro da janela principal.
pub fn centered_movable_window<'a>(
    title: impl Into<egui::WidgetText>,
    id_salt: &'static str,
    ctx: &egui::Context,
    default_size: Vec2,
) -> egui::Window<'a> {
    let sr = ctx.screen_rect();
    let default_pos = egui::pos2(
        (sr.center().x - default_size.x * 0.5).max(sr.left() + 8.0),
        (sr.center().y - default_size.y * 0.5).max(sr.top() + 8.0),
    );
    egui::Window::new(title)
        .id(egui::Id::new(id_salt))
        .default_pos(default_pos)
        .movable(true)
        .constrain_to(sr)
        .collapsible(false)
        .resizable(false)
}
