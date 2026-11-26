use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;

/// Gerencia texturas de video por ID de tile sem alocar novas texturas GPU a cada quadro.
pub struct VideoTexturePool {
    textures: HashMap<String, TextureHandle>,
}

impl VideoTexturePool {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    pub fn update_texture(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        width: usize,
        height: usize,
        rgba: &[u8],
    ) -> Option<&TextureHandle> {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return None;
        }
        let img = ColorImage::from_rgba_unmultiplied([width, height], &rgba[..width * height * 4]);
        if let Some(handle) = self.textures.get_mut(key) {
            handle.set(img, TextureOptions::LINEAR);
        } else {
            let handle = ctx.load_texture(key, img, TextureOptions::LINEAR);
            self.textures.insert(key.to_string(), handle);
        }
        self.textures.get(key)
    }

    pub fn update_color_image(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        width: usize,
        height: usize,
        pixels: Vec<egui::Color32>,
    ) -> Option<&TextureHandle> {
        if width == 0 || height == 0 || pixels.len() != width * height {
            return None;
        }
        let img = ColorImage {
            size: [width, height],
            pixels,
        };
        if let Some(handle) = self.textures.get_mut(key) {
            handle.set(img, TextureOptions::LINEAR);
        } else {
            let handle = ctx.load_texture(key, img, TextureOptions::LINEAR);
            self.textures.insert(key.to_string(), handle);
        }
        self.textures.get(key)
    }

    pub fn get(&self, key: &str) -> Option<&TextureHandle> {
        self.textures.get(key)
    }

    pub fn remove(&mut self, key: &str) {
        self.textures.remove(key);
        if self.textures.is_empty() {
            self.textures.shrink_to_fit();
        }
    }

    pub fn remove_peer_textures(&mut self, peer_id: &str) {
        let suffix = format!(":{}", peer_id);
        self.textures
            .retain(|k, _| !(k.starts_with("remote:") && k.ends_with(&suffix)));
        if self.textures.is_empty() {
            self.textures.shrink_to_fit();
        }
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        self.textures.shrink_to_fit();
    }
}
