use super::*;
use ateliet_core::board::pages as pg;

fn editor(rtl: bool, frame: usize) -> BoardEditor {
    // Deliberately not a multiple of TILE: the spine cuts through a tile.
    let mut d = BoardDoc::new("comic", 650, 360, "#ffffff", 24);
    d.board["page"] = json!({ "spreads": true, "rtl": rtl, "mask": false });
    d.add_frame(1);
    d.add_frame(2);
    let mut e = BoardEditor::with("test.storyboard", PathBuf::new(), d);
    e.frame = frame;
    e.toggle_joined();
    e.undo.clear();
    for s in e.settings.values_mut() { s.smoothing = 0.0; s.size = 12.0; }
    e
}

fn alpha(e: &mut BoardEditor, frame: usize, x: i32, y: i32) -> u8 {
    let id = e.doc.active_layer(frame);
    let lx = x - e.doc.layer(&id)["x"].as_i64().unwrap_or(0) as i32;
    let ly = y - e.doc.layer(&id)["y"].as_i64().unwrap_or(0) as i32;
    e.doc.tile(&id, lx.div_euclid(TILE as i32), ly.div_euclid(TILE as i32))
        .map(|t| t.px[(ly.rem_euclid(TILE as i32) as usize * TILE + lx.rem_euclid(TILE as i32) as usize) * 4 + 3]).unwrap_or(0)
}

fn line(e: &mut BoardEditor, a: egui::Pos2, b: egui::Pos2) {
    e.begin(a).unwrap();
    let dabs = e.active.as_mut().unwrap().stroke.add(Point { x: b.x, y: b.y, p: 1.0 });
    e.paint(dabs);
}

#[test]
fn both_reading_directions_and_both_active_pages_cross_the_spine() {
    for rtl in [false, true] {
        for frame in [1, 2] {
            let mut e = editor(rtl, frame);
            let (j, off) = e.joined_facing().unwrap();
            let edge = if off > 0.0 { 650.0 } else { 0.0 };
            for kind in BrushKind::ALL.into_iter().filter(|k| *k != BrushKind::Eraser) {
                e.tool = Tool::Brush(kind);
                line(&mut e, egui::pos2(edge - 20.0, 90.0), egui::pos2(edge + 20.0, 90.0));
                // Both pages composite the ongoing stroke before pen-up.
                let ctx = egui::Context::default();
                let fk = (j, if off > 0.0 { 0 } else { 1 }, 0);
                let _ = e.composite(&ctx, fk);
                let active = e.active.as_ref().unwrap();
                assert_eq!(active.surfaces.len(), 2);
                assert!(active.surfaces.iter().all(|s| !s.before.is_empty() || !s.buffer.is_empty()));
                e.finish(Some(egui::pos2(edge + 20.0, 90.0)));
                assert!(alpha(&mut e, frame, if off > 0.0 { 640 } else { 10 }, 90) > 0, "{rtl} {frame} {kind:?}");
                assert!(alpha(&mut e, j, if off > 0.0 { 10 } else { 640 }, 90) > 0);
                assert_eq!(alpha(&mut e, frame, -1, 90), 0);
                assert_eq!(alpha(&mut e, frame, 650, 90), 0);
                e.undo();
            }
        }
    }
}

#[test]
fn eraser_previews_and_commits_on_both_pages_with_one_undo() {
    let mut e = editor(false, 1);
    e.tool = Tool::Brush(BrushKind::Pen);
    line(&mut e, egui::pos2(630.0, 100.0), egui::pos2(670.0, 100.0));
    e.finish(Some(egui::pos2(670.0, 100.0)));
    e.tool = Tool::Brush(BrushKind::Eraser);
    line(&mut e, egui::pos2(630.0, 100.0), egui::pos2(670.0, 100.0));
    let a = e.active.as_ref().unwrap();
    let facing = a.surfaces.iter().find(|s| s.frame == 2).unwrap();
    let ov = Overlay { layer: &facing.layer, tiles: &facing.buffer, opacity: 1.0, erase: true, alpha_lock: false };
    let px = compose::region(&mut e.doc, &mut e.pics, 2, 10, 100, 1, 1, &Options { background: false, onion: false, overlay: Some(ov), exporting: false });
    assert_eq!(px[3], 0, "eraser must preview before pen-up");
    e.finish(Some(egui::pos2(670.0, 100.0)));
    assert_eq!(alpha(&mut e, 1, 640, 100), 0);
    assert_eq!(alpha(&mut e, 2, 10, 100), 0);
    assert_eq!(e.undo.len(), 2);
    e.undo();
    assert!(alpha(&mut e, 1, 640, 100) > 0);
    assert!(alpha(&mut e, 2, 10, 100) > 0);
    e.redo();
    assert_eq!(alpha(&mut e, 2, 10, 100), 0);
}

#[test]
fn offsets_opacity_locked_layers_and_split_round_trip() {
    let mut e = editor(false, 1);
    let target = e.doc.active_layer(2);
    e.doc.layer_mut(&target)["x"] = json!(17);
    e.doc.layer_mut(&target)["y"] = json!(-9);
    e.tool = Tool::Brush(BrushKind::Pen);
    e.settings.get_mut(&BrushKind::Pen).unwrap().opacity = 0.5;
    line(&mut e, egui::pos2(640.0, 70.0), egui::pos2(670.0, 70.0));
    e.finish(Some(egui::pos2(670.0, 70.0)));
    assert!((125..=129).contains(&alpha(&mut e, 2, 10, 70)));
    e.toggle_joined();
    assert!(!e.is_joined());
    let bytes = e.doc.to_bytes(|_, _| None);
    e.doc = BoardDoc::from_bytes(&bytes).unwrap();
    assert!((125..=129).contains(&alpha(&mut e, 2, 10, 70)));
    e.toggle_joined();
    e.doc.layer_mut(&target)["locked"] = json!(true);
    line(&mut e, egui::pos2(640.0, 150.0), egui::pos2(670.0, 150.0));
    e.finish(Some(egui::pos2(670.0, 150.0)));
    assert_eq!(alpha(&mut e, 2, 10, 150), 0);
}

#[test]
fn joining_does_not_retarget_after_reorder() {
    let mut e = editor(false, 1);
    e.doc.add_frame(3);
    e.doc.move_frame(2, 3);
    assert!(!e.is_joined());
    e.doc.move_frame(3, 2);
    assert!(e.is_joined());
}

#[test]
fn pointer_down_draws_a_dot_and_switches_page_without_a_second_click() {
    let mut e = editor(false, 1);
    e.toggle_joined();
    e.tool = Tool::Brush(BrushKind::Pen);
    e.zoom = 1.0;
    e.fit = false;
    let ctx = egui::Context::default();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 600.0));
    let mut point = egui::Pos2::ZERO;
    let p = crate::theme::Palette::new(&crate::config::Appearance::default());
    let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), ..Default::default() }, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            point = e.origin(ui.available_rect_before_wrap()) + egui::vec2(660.0, 100.0);
            e.canvas_ui(ui, p);
        });
    });
    let events = [true, false].into_iter().map(|pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }).collect();
    let _ = ctx.run(egui::RawInput { screen_rect: Some(bounds), events, ..Default::default() }, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| { e.canvas_ui(ui, p); });
    });
    assert_eq!(e.frame, 2);
    assert!(e.active.is_none());
    assert!(alpha(&mut e, 2, 10, 100) > 0);
    assert_eq!(alpha(&mut e, 1, 10, 100), 0);
}

#[test]
fn live_paint_invalidates_only_touched_tiles() {
    let mut e = editor(false, 1);
    e.stale.clear();
    e.tool = Tool::Brush(BrushKind::Pen);
    e.begin(egui::pos2(645.0, 90.0)).unwrap();
    assert_eq!(e.stale, HashSet::from([(1, 1, 0), (2, 0, 0)]));
    e.finish(None);
    assert_eq!(e.stale.len(), 2, "pen-up must not invalidate the entire spread");
}

#[test]
fn layout_respects_binding_first_side_and_unpaired_edges() {
    for rtl in [false, true] {
        for first in ["left", "right"] {
            let b = json!({ "page": { "spreads": true, "rtl": rtl, "firstSide": first } });
            let alone = (first == "left") == rtl;
            assert_eq!(pg::facing(&b, 0, 5), if alone { None } else { Some(1) });
            assert_eq!(pg::is_left(&b, 0), first == "left");
            for i in 0..5 {
                if let Some(j) = pg::facing(&b, i, 5) {
                    assert_eq!(pg::facing(&b, j, 5), Some(i));
                    assert_ne!(pg::is_left(&b, i), pg::is_left(&b, j));
                }
            }
        }
        let b = json!({ "page": { "spreads": true, "rtl": rtl } });
        assert_eq!(pg::facing(&b, 0, 4), None);
        assert_eq!(pg::facing(&b, 1, 4), Some(2));
        assert_eq!(pg::facing(&b, 3, 4), None);
        assert_eq!(pg::facing(&b, 4, 4), None);
    }
}

#[test]
fn alpha_lock_does_not_create_ink_on_the_facing_page() {
    for kind in [BrushKind::Pencil, BrushKind::Pen] {
        let mut e = editor(false, 1);
        let target = e.doc.active_layer(2);
        e.doc.layer_mut(&target)["alphaLock"] = json!(true);
        e.tool = Tool::Brush(kind);
        line(&mut e, egui::pos2(640.0, 90.0), egui::pos2(670.0, 90.0));
        e.finish(Some(egui::pos2(670.0, 90.0)));
        assert_eq!(alpha(&mut e, 2, 10, 90), 0);
        assert!(alpha(&mut e, 1, 640, 90) > 0);
    }
}

#[test]
fn frame_snapshot_is_independent_and_keeps_only_its_own_layers() {
    let mut e = editor(false, 1);
    e.tool = Tool::Brush(BrushKind::Pencil);
    e.begin(egui::pos2(90.0, 90.0)).unwrap();
    e.finish(None);
    let mut snapshot = e.doc.frame_snapshot(1);
    assert_eq!(snapshot.frame_count(), 1);
    let layer = snapshot.active_layer(0);
    assert_eq!(snapshot.board["layers"].as_object().unwrap().len(), 1);
    assert!(snapshot.tiles.keys().all(|k| k.0 == layer));
    snapshot.tile_mut(&layer, 0, 0).px.fill(0);
    assert!(alpha(&mut e, 1, 90, 90) > 0);
}

#[test]
#[ignore = "manual CPU comparison; timing is not a test assertion"]
fn spread_render_cost() {
    let mut e = editor(false, 1);
    e.doc.board["width"] = json!(2480);
    e.doc.board["height"] = json!(3508);
    let db = crate::fonts::book_fonts();
    let start = Instant::now();
    let _ = pg::page_image(&mut e.doc, &mut e.pics, &db, 2, 1.0, false);
    let whole = start.elapsed();
    let ctx = egui::Context::default();
    let start = Instant::now();
    for _ in 0..10 { let _ = e.composite(&ctx, (2, 0, 0)); }
    let tile = start.elapsed() / 10;
    println!("2480×3508 facing page: full page {:.2} ms; dirty 512×512 tile {:.2} ms; CPU ratio {:.1}×", whole.as_secs_f64() * 1000.0, tile.as_secs_f64() * 1000.0, whole.as_secs_f64() / tile.as_secs_f64());
}
