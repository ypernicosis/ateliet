use super::*;
use eframe::App as _;

fn seeded() -> Video {
    let mut video = Video::default();
    video.history = History::default();
    video.current = Some(Recent { source: Source::Youtube { id: "dQw4w9WgXcQ".into() }, title: "Referência de composição e movimento".into(), position: 92.0 });
    video.presentation = Presentation::Docked;
    video.switcher = false;
    video
}

fn workspace(name: &str) -> (crate::app::App, egui::Context, PathBuf) {
    let ctx = egui::Context::default();
    let mut app = crate::app::App::new(&eframe::CreationContext::_new_kittest(ctx.clone()), None);
    let root = std::env::temp_dir().join(format!("ateliet-video-layout-{}-{name}", std::process::id()));
    std::fs::create_dir_all(root.join("Cadernos")).unwrap();
    std::fs::write(root.join("Cadernos/Ideias.md"), "# O ritmo da cena\n\nA luz atravessa a janela. É o primeiro movimento da manhã.\n\n## Referências\n\nObservar a composição, o espaço vazio e o tempo de cada gesto.\n\n## Próximo passo\n\nEscrever a cena mantendo o vídeo ao lado.\n").unwrap();
    crate::images_ui::ImagesState::save_settings(&root, serde_json::json!({"shared":"", "show_vault":true}));
    app.vault = Some(crate::vault::Vault::open(&root, ctx.clone()));
    app.open("Cadernos/Ideias.md", true);
    app.video = seeded();
    (app, ctx, root)
}

fn frames(app: &mut crate::app::App, ctx: &egui::Context, size: [f32; 2]) -> Vec<egui::FullOutput> {
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    (0..4).map(|n| ctx.run(egui::RawInput { screen_rect: Some(bounds), time: Some(n as f64 + 1.0), ..Default::default() }, |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()))).collect()
}

#[test]
fn one_session_follows_every_workspace_without_covering_the_editor() {
    let (mut app, ctx, root) = workspace("modes");
    let source = app.video.current.as_ref().unwrap().source.clone();
    for mode in ["notes", "images", "journal", "reader-beside", "focus", "immersive"] {
        app.images_mode = mode == "images";
        app.journal_mode = mode == "journal";
        app.focus = (mode == "focus").then(crate::sc::focus::Focus::new);
        app.side_reader = (mode == "reader-beside").then(|| Box::new(crate::reader_ui::Reader::new(crate::reader_ui::SHELF, true)));
        app.right_open = app.side_reader.is_none();
        app.tabs[0].reader = if mode == "immersive" {
            let mut reader = crate::reader_ui::Reader::new(crate::reader_ui::SHELF, false);
            reader.immersive = true; Some(Box::new(reader))
        } else { None };
        frames(&mut app, &ctx, [1280.0, 820.0]);
        let rect = app.video.player_rect.unwrap_or_else(|| panic!("missing player in {mode}"));
        assert!(rect.width() >= 200.0 && rect.height() >= 200.0, "{mode}: {rect:?}");
        assert!(ctx.screen_rect().contains_rect(rect), "{mode}: {rect:?}");
        assert!(!ctx.available_rect().intersects(rect), "video must reserve layout space in {mode}");
        assert_eq!(app.video.unobscured_rect(&ctx), Some(rect), "{mode}: unexpected occlusion");
        assert_eq!(app.video.current.as_ref().unwrap().source, source);
        assert_eq!(app.video.current.as_ref().unwrap().position, 92.0);
        assert!(app.video.backend.is_none(), "layout must not load a decoder");
    }
    drop(app); let _ = std::fs::remove_dir_all(root);
}

#[test]
fn collapsing_restores_space_and_keeps_sidebar_preferences() {
    let (mut app, ctx, root) = workspace("collapse");
    app.right_open = false;
    frames(&mut app, &ctx, [640.0, 400.0]);
    let docked = ctx.available_rect();
    assert!(docked.width() >= 310.0, "small workspace: {docked:?}");
    assert!(app.left_open && !app.right_open);
    assert!(app.video.player_rect.is_some());
    app.video.collapse();
    frames(&mut app, &ctx, [1280.0, 820.0]);
    assert_eq!(app.video.presentation, Presentation::Collapsed);
    assert!(app.video.player_rect.is_none());
    assert!(app.left_open && !app.right_open);
    assert!(ctx.available_rect().width() > 700.0);
    assert!(ctx.available_rect().bottom() < 820.0 - 30.0, "compact control must reserve its own strip");
    app.video.reveal();
    frames(&mut app, &ctx, [1280.0, 820.0]);
    assert!(app.video.player_rect.is_some());
    assert_eq!(app.video.current.as_ref().unwrap().position, 92.0);
    assert!(app.left_open && !app.right_open);
    drop(app); let _ = std::fs::remove_dir_all(root);
}

#[test]
fn inline_selector_fits_small_workspace_with_long_recent_titles() {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    let mut video = Video::default();
    video.history = History::default();
    for i in 0..8 {
        video.history.remember(Source::Local { path: PathBuf::from(format!("/example/{i}.mov")) }, "Uma referência com um título muito longo ".repeat(10), 42.0);
    }
    video.reveal();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ctx| {
            video.rail(ctx, Palette::new(&Default::default()));
            egui::CentralPanel::default().show(ctx, |ui| { ui.label("Editor"); });
        });
    }
    assert!(bounds.contains_rect(video.rect.unwrap()));
    assert!(ctx.memory(|m| m.areas().visible_layer_ids().iter().all(|l| l.order == egui::Order::Background)), "selector must not create a floating egui window");
    assert!(video.backend.is_none());
    video.collapse();
    assert_eq!(video.presentation, Presentation::Closed);
}

#[test]
fn overlapping_dialog_hides_native_surface_and_nonoverlapping_dialog_does_not() {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    let mut video = seeded();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    for x in [720.0, 20.0] {
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ctx| {
                video.rail(ctx, Palette::new(&Default::default()));
                egui::CentralPanel::default().show(ctx, |_| {});
                egui::Window::new("Test dialog").fixed_pos([x, 70.0]).fixed_size([210.0, 220.0]).show(ctx, |ui| { ui.label("Configurações"); });
            });
        }
        assert_eq!(video.unobscured_rect(&ctx).is_none(), x > 700.0);
    }
}

/// CPU snapshot of the real egui layout. Native video is deliberately a
/// placeholder: this validates composition, not media decoding or playback.
#[test]
#[ignore = "writes visual review PNGs when ATELIET_VIDEO_PREVIEW is set"]
fn render_layout_preview() {
    use std::collections::HashMap;
    use egui::epaint::{ImageData, Primitive};
    let out = PathBuf::from(std::env::var("ATELIET_VIDEO_PREVIEW").expect("preview output directory"));
    std::fs::create_dir_all(&out).unwrap();
    let (mut app, ctx, root) = workspace("preview");
    app.vault.as_mut().unwrap().name = "ocean".into();
    let mut textures: HashMap<egui::TextureId, egui::ColorImage> = HashMap::new();
    for name in ["encaixado", "trocar-video", "recolhido", "imagens", "pequeno"] {
        if name == "trocar-video" { app.video.switcher = true; }
        if name == "recolhido" { app.video.collapse(); }
        if name == "imagens" { app.images_mode = true; app.video.reveal(); }
        if name == "pequeno" { app.images_mode = false; }
        let size = if name == "pequeno" { [640.0, 400.0] } else { [1280.0, 820.0] };
        let outputs = frames(&mut app, &ctx, size);
        let mut shapes = Vec::new();
        let mut pixels_per_point = 1.0;
        for output in outputs {
            for (id, delta) in output.textures_delta.set {
                let patch = match delta.image {
                    ImageData::Color(image) => (*image).clone(),
                    ImageData::Font(image) => egui::ColorImage { size: image.size, pixels: image.srgba_pixels(None).collect() },
                };
                if let Some([x, y]) = delta.pos {
                    let image = textures.get_mut(&id).unwrap();
                    for row in 0..patch.size[1] {
                        let start = (y + row) * image.size[0] + x;
                        image.pixels[start..start + patch.size[0]].copy_from_slice(&patch.pixels[row * patch.size[0]..(row + 1) * patch.size[0]]);
                    }
                } else { textures.insert(id, patch); }
            }
            shapes = output.shapes; pixels_per_point = output.pixels_per_point;
        }
        let width = (size[0] * pixels_per_point) as u32;
        let height = (size[1] * pixels_per_point) as u32;
        let mut canvas = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
        for clipped in ctx.tessellate(shapes, pixels_per_point) {
            let Primitive::Mesh(mesh) = clipped.primitive else { continue };
            let texture = &textures[&mesh.texture_id];
            let clip = clipped.clip_rect * pixels_per_point;
            for tri in mesh.indices.chunks_exact(3) {
                let v = tri.iter().map(|i| mesh.vertices[*i as usize]).collect::<Vec<_>>();
                let q = v.iter().map(|v| v.pos * pixels_per_point).collect::<Vec<_>>();
                let det = (q[1].y-q[2].y)*(q[0].x-q[2].x)+(q[2].x-q[1].x)*(q[0].y-q[2].y);
                if det.abs() < 0.001 { continue; }
                let min_x = q.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).max(clip.min.x).max(0.0).floor() as u32;
                let min_y = q.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).max(clip.min.y).max(0.0).floor() as u32;
                let max_x = q.iter().map(|p| p.x).fold(0.0, f32::max).min(clip.max.x).min(width as f32).ceil() as u32;
                let max_y = q.iter().map(|p| p.y).fold(0.0, f32::max).min(clip.max.y).min(height as f32).ceil() as u32;
                for y in min_y..max_y { for x in min_x..max_x {
                    let (px, py) = (x as f32+0.5, y as f32+0.5);
                    let a = ((q[1].y-q[2].y)*(px-q[2].x)+(q[2].x-q[1].x)*(py-q[2].y))/det;
                    let b = ((q[2].y-q[0].y)*(px-q[2].x)+(q[0].x-q[2].x)*(py-q[2].y))/det;
                    let c = 1.0-a-b;
                    if a < 0.0 || b < 0.0 || c < 0.0 { continue; }
                    let uv = v[0].uv.to_vec2()*a+v[1].uv.to_vec2()*b+v[2].uv.to_vec2()*c;
                    let tx = ((uv.x*texture.size[0] as f32) as usize).min(texture.size[0]-1);
                    let ty = ((uv.y*texture.size[1] as f32) as usize).min(texture.size[1]-1);
                    let texel = texture.pixels[ty*texture.size[0]+tx].to_array();
                    let mut color = [0.0; 4];
                    for k in 0..4 { color[k] = (v[0].color[k] as f32*a+v[1].color[k] as f32*b+v[2].color[k] as f32*c)*texel[k] as f32/255.0; }
                    let dest = canvas.get_pixel_mut(x, y);
                    for k in 0..3 { dest[k] = (color[k]+dest[k] as f32*(1.0-color[3]/255.0)).min(255.0) as u8; }
                }}
            }
        }
        canvas.save(out.join(format!("{name}.png"))).unwrap();
    }
    drop(app); let _ = std::fs::remove_dir_all(root);
}

#[test]
fn source_switcher_does_not_expand_the_support_panel_over_time() {
    let (mut app, ctx, root) = workspace("stable-width");
    frames(&mut app, &ctx, [1280.0, 820.0]);
    let initial = app.video.player_rect.unwrap();
    app.video.switcher = true;
    for _ in 0..5 { frames(&mut app, &ctx, [1280.0, 820.0]); }
    let final_rect = app.video.player_rect.unwrap();
    assert!((initial.width() - final_rect.width()).abs() < 1.0, "{initial:?} -> {final_rect:?}");
    assert!((initial.left() - final_rect.left()).abs() < 1.0);
    drop(app); let _ = std::fs::remove_dir_all(root);
}

#[test]
fn focus_controls_stay_above_the_collapsed_video_strip() {
    let (mut app, ctx, root) = workspace("focus-strip");
    app.focus = Some(crate::sc::focus::Focus::new());
    app.video.collapse();
    frames(&mut app, &ctx, [1280.0, 820.0]);
    let hud = ctx.memory(|m| m.area_rect(egui::Id::new("focus-hud"))).unwrap();
    assert!(hud.bottom() <= ctx.available_rect().bottom(), "{hud:?}");
    assert!(hud.bottom() < 820.0 - 36.0);
    drop(app); let _ = std::fs::remove_dir_all(root);
}
