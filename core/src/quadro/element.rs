//! Drawing elements in Excalidraw's own JSON. An element is kept as the
//! object it was read from, so fields this code does not know survive a
//! save; the accessors read and write the ones it draws and edits.

use serde_json::{json, Map, Value};

#[derive(Clone, Debug)]
pub struct Element {
    pub v: Map<String, Value>,
    /// Bumped by every change (caches of drawn geometry key on it).
    pub rev: u64,
    /// Changed since it was read: its `version` goes up when saved.
    pub touched: bool,
}

/// The look new elements get (the toolbar's current style).
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub stroke: String,
    pub background: String,
    pub fill: String,
    pub width: f32,
    pub dash: String,
    pub roughness: f32,
    pub opacity: f32,
    pub font_size: f32,
    pub font_family: u32,
    pub align: String,
    pub start_head: Option<String>,
    pub end_head: Option<String>,
    /// Rounded corners for rectangles and diamonds, curves for arrows.
    pub round: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            stroke: "#1e1e1e".into(),
            background: "transparent".into(),
            fill: "hachure".into(),
            width: 2.0,
            dash: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            font_size: 20.0,
            font_family: 5,
            align: "left".into(),
            start_head: None,
            end_head: Some("arrow".into()),
            round: true,
        }
    }
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x2545_f491);

/// A fresh pseudo-random number (time and a counter, mixed).
pub fn random_u64() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut x = t ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, std::sync::atomic::Ordering::Relaxed);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// An 8 character id: the length the Excalidraw plugin's block references
/// (`text ^abcd1234`) expect.
pub fn new_id() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut x = random_u64();
    (0..8)
        .map(|_| {
            let c = A[(x % A.len() as u64) as usize] as char;
            x /= A.len() as u64;
            if x == 0 {
                x = random_u64();
            }
            c
        })
        .collect()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn num(v: f32) -> Value {
    // whole numbers as integers, as JavaScript prints them
    if v.fract() == 0.0 && v.abs() < 1e15 {
        json!(v as i64)
    } else {
        json!((v as f64 * 1000.0).round() / 1000.0)
    }
}

impl Element {
    pub fn from_value(v: Value) -> Option<Element> {
        match v {
            Value::Object(m) => Some(Element { v: m, rev: 0, touched: false }),
            _ => None,
        }
    }

    /// A new element of `kind` with Excalidraw's defaults and `style`.
    pub fn new(kind: &str, x: f32, y: f32, w: f32, h: f32, style: &Style) -> Element {
        let seed = (random_u64() % 2_000_000_000) as i64;
        let mut v = json!({
            "id": new_id(),
            "type": kind,
            "x": num(x),
            "y": num(y),
            "width": num(w),
            "height": num(h),
            "angle": 0,
            "strokeColor": style.stroke,
            "backgroundColor": style.background,
            "fillStyle": style.fill,
            "strokeWidth": num(style.width),
            "strokeStyle": style.dash,
            "roughness": num(style.roughness),
            "opacity": num(style.opacity),
            "groupIds": [],
            "frameId": null,
            "roundness": null,
            "seed": seed,
            "version": 1,
            "versionNonce": (random_u64() % 2_000_000_000) as i64,
            "isDeleted": false,
            "boundElements": null,
            "updated": now_ms(),
            "link": null,
            "locked": false
        });
        let m = v.as_object_mut().unwrap();
        match kind {
            "rectangle" | "diamond" if style.round => {
                m.insert("roundness".into(), json!({ "type": 3 }));
            }
            "arrow" | "line" => {
                if style.round {
                    m.insert("roundness".into(), json!({ "type": 2 }));
                }
                m.insert("points".into(), json!([[0, 0], [num(w), num(h)]]));
                m.insert("lastCommittedPoint".into(), Value::Null);
                m.insert("startBinding".into(), Value::Null);
                m.insert("endBinding".into(), Value::Null);
                if kind == "arrow" {
                    m.insert("startArrowhead".into(), json!(style.start_head));
                    m.insert("endArrowhead".into(), json!(style.end_head));
                    m.insert("elbowed".into(), json!(false));
                } else {
                    m.insert("startArrowhead".into(), Value::Null);
                    m.insert("endArrowhead".into(), Value::Null);
                }
            }
            "freedraw" => {
                m.insert("points".into(), json!([[0, 0]]));
                m.insert("pressures".into(), json!([]));
                m.insert("simulatePressure".into(), json!(true));
                m.insert("lastCommittedPoint".into(), Value::Null);
            }
            "text" => {
                m.insert("text".into(), json!(""));
                m.insert("rawText".into(), json!(""));
                m.insert("fontSize".into(), num(style.font_size));
                m.insert("fontFamily".into(), json!(style.font_family));
                m.insert("textAlign".into(), json!(style.align));
                m.insert("verticalAlign".into(), json!("top"));
                m.insert("containerId".into(), Value::Null);
                m.insert("originalText".into(), json!(""));
                m.insert("autoResize".into(), json!(true));
                m.insert("lineHeight".into(), json!(1.25));
            }
            "image" => {
                m.insert("fileId".into(), Value::Null);
                m.insert("status".into(), json!("saved"));
                m.insert("scale".into(), json!([1, 1]));
                m.insert("crop".into(), Value::Null);
            }
            _ => {}
        }
        Element { v: v.as_object().cloned().unwrap_or_default(), rev: 0, touched: true }
    }

    pub fn to_value(&self) -> Value {
        Value::Object(self.v.clone())
    }

    pub fn id(&self) -> &str {
        self.v.get("id").and_then(|v| v.as_str()).unwrap_or("")
    }
    pub fn kind(&self) -> &str {
        self.v.get("type").and_then(|v| v.as_str()).unwrap_or("")
    }
    pub fn f(&self, k: &str) -> f32 {
        self.v.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32
    }
    pub fn fo(&self, k: &str, d: f32) -> f32 {
        self.v.get(k).and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(d)
    }
    pub fn s(&self, k: &str) -> &str {
        self.v.get(k).and_then(|v| v.as_str()).unwrap_or("")
    }
    pub fn get(&self, k: &str) -> &Value {
        self.v.get(k).unwrap_or(&Value::Null)
    }

    pub fn set(&mut self, k: &str, v: Value) {
        if self.v.get(k) != Some(&v) {
            self.v.insert(k.to_string(), v);
            self.rev += 1;
            self.touched = true;
        }
    }
    pub fn set_f(&mut self, k: &str, v: f32) {
        self.set(k, num(v));
    }

    pub fn x(&self) -> f32 {
        self.f("x")
    }
    pub fn y(&self) -> f32 {
        self.f("y")
    }
    pub fn w(&self) -> f32 {
        self.f("width")
    }
    pub fn h(&self) -> f32 {
        self.f("height")
    }
    pub fn angle(&self) -> f32 {
        self.f("angle")
    }
    pub fn deleted(&self) -> bool {
        self.get("isDeleted").as_bool().unwrap_or(false)
    }
    pub fn locked(&self) -> bool {
        self.get("locked").as_bool().unwrap_or(false)
    }
    pub fn seed(&self) -> u32 {
        self.get("seed").as_f64().unwrap_or(1.0) as u32
    }
    pub fn container(&self) -> Option<&str> {
        self.get("containerId").as_str().filter(|s| !s.is_empty())
    }
    pub fn group_ids(&self) -> Vec<String> {
        self.get("groupIds").as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
    pub fn is_linear(&self) -> bool {
        matches!(self.kind(), "line" | "arrow" | "freedraw")
    }

    /// The text as written (with `[[links]]` unparsed, as the plugin keeps it).
    pub fn raw_text(&self) -> &str {
        ["rawText", "originalText", "text"].iter().map(|k| self.s(k)).find(|s| !s.is_empty()).unwrap_or("")
    }

    pub fn set_text(&mut self, t: &str) {
        for k in ["text", "originalText", "rawText"] {
            self.set(k, json!(t));
        }
    }

    /// Points relative to (x, y).
    pub fn points(&self) -> Vec<(f32, f32)> {
        self.get("points").as_array().map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_f64()? as f32, p.get(1)?.as_f64()? as f32))).collect()).unwrap_or_default()
    }

    /// Set the points (absolute), keeping the first one at (0, 0) of the
    /// element as Excalidraw does, and the box around them.
    pub fn set_abs_points(&mut self, pts: &[(f32, f32)]) {
        let Some(&(x0, y0)) = pts.first() else { return };
        let rel: Vec<Value> = pts.iter().map(|(x, y)| json!([num(x - x0), num(y - y0)])).collect();
        let (mut a, mut b) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for (x, y) in pts {
            a = (a.0.min(*x), a.1.min(*y));
            b = (b.0.max(*x), b.1.max(*y));
        }
        self.set_f("x", x0);
        self.set_f("y", y0);
        self.set("points", Value::Array(rel));
        self.set_f("width", b.0 - a.0);
        self.set_f("height", b.1 - a.1);
    }

    pub fn abs_points(&self) -> Vec<(f32, f32)> {
        let (x, y) = (self.x(), self.y());
        self.points().into_iter().map(|(a, b)| (a + x, b + y)).collect()
    }

    /// The unrotated box: (x0, y0, x1, y1).
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        if self.is_linear() {
            let pts = self.abs_points();
            if pts.is_empty() {
                return (self.x(), self.y(), self.x(), self.y());
            }
            let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in pts {
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
            return b;
        }
        let (x, y, w, h) = (self.x(), self.y(), self.w(), self.h());
        (x.min(x + w), y.min(y + h), x.max(x + w), y.max(y + h))
    }

    pub fn center(&self) -> (f32, f32) {
        let b = self.bounds();
        ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)
    }

    /// The box around the element as drawn (rotation included).
    pub fn aabb(&self) -> (f32, f32, f32, f32) {
        let b = self.bounds();
        let a = self.angle();
        if a == 0.0 {
            return b;
        }
        let c = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
        let mut out = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in [(b.0, b.1), (b.2, b.1), (b.2, b.3), (b.0, b.3)] {
            let q = rotate(p, c, a);
            out = (out.0.min(q.0), out.1.min(q.1), out.2.max(q.0), out.3.max(q.1));
        }
        out
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        let (x, y) = (self.x(), self.y());
        self.set_f("x", x + dx);
        self.set_f("y", y + dy);
    }

    /// Does a point (world) touch the element, within `tol`?
    pub fn hit(&self, px: f32, py: f32, tol: f32) -> bool {
        let c = self.center();
        let (px, py) = rotate((px, py), c, -self.angle());
        let b = self.bounds();
        let filled = self.s("backgroundColor") != "transparent" && !self.s("backgroundColor").is_empty();
        match self.kind() {
            "line" | "arrow" | "freedraw" => {
                let pts = self.abs_points();
                if pts.len() == 1 {
                    return dist(pts[0], (px, py)) <= tol + self.f("strokeWidth");
                }
                if filled && pts.len() > 2 && dist(pts[0], *pts.last().unwrap()) < 8.0 && inside(&pts, (px, py)) {
                    return true;
                }
                pts.windows(2).any(|s| seg_dist((px, py), s[0], s[1]) <= tol + self.f("strokeWidth") / 2.0)
            }
            "ellipse" => {
                let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
                let (rx, ry) = (((b.2 - b.0) / 2.0).max(0.5), ((b.3 - b.1) / 2.0).max(0.5));
                let d = (((px - cx) / rx).powi(2) + ((py - cy) / ry).powi(2)).sqrt();
                if filled {
                    return d <= 1.0 + tol / rx.min(ry);
                }
                (d - 1.0).abs() * rx.min(ry) <= tol
            }
            "diamond" => {
                let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
                let pts = [(cx, b.1), (b.2, cy), (cx, b.3), (b.0, cy)];
                if filled && inside(&pts, (px, py)) {
                    return true;
                }
                (0..4).any(|i| seg_dist((px, py), pts[i], pts[(i + 1) % 4]) <= tol)
            }
            "rectangle" | "frame" | "magicframe" | "embeddable" | "iframe" => {
                let inside_box = px >= b.0 - tol && px <= b.2 + tol && py >= b.1 - tol && py <= b.3 + tol;
                if !inside_box {
                    return false;
                }
                if filled || matches!(self.kind(), "embeddable" | "iframe") {
                    return true;
                }
                let near = (px - b.0).abs() <= tol || (px - b.2).abs() <= tol || (py - b.1).abs() <= tol || (py - b.3).abs() <= tol;
                // a frame is grabbed by its name too
                near || (matches!(self.kind(), "frame" | "magicframe") && py < b.1 + 4.0)
            }
            _ => px >= b.0 - tol && px <= b.2 + tol && py >= b.1 - tol && py <= b.3 + tol,
        }
    }

    /// Ids of the elements bound to this one (`boundElements`).
    pub fn bound(&self) -> Vec<(String, String)> {
        self.get("boundElements").as_array().map(|a| a.iter().filter_map(|b| Some((b.get("id")?.as_str()?.to_string(), b.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string()))).collect()).unwrap_or_default()
    }

    pub fn add_bound(&mut self, id: &str, kind: &str) {
        let mut list = self.get("boundElements").as_array().cloned().unwrap_or_default();
        if !list.iter().any(|b| b.get("id").and_then(|x| x.as_str()) == Some(id)) {
            list.push(json!({ "id": id, "type": kind }));
            self.set("boundElements", Value::Array(list));
        }
    }

    pub fn remove_bound(&mut self, id: &str) {
        let Some(list) = self.get("boundElements").as_array().cloned() else { return };
        let kept: Vec<Value> = list.into_iter().filter(|b| b.get("id").and_then(|x| x.as_str()) != Some(id)).collect();
        self.set("boundElements", if kept.is_empty() { Value::Null } else { Value::Array(kept) });
    }

    /// The element an arrow end is bound to.
    pub fn binding(&self, end: bool) -> Option<String> {
        self.get(if end { "endBinding" } else { "startBinding" }).get("elementId").and_then(|v| v.as_str()).map(str::to_string)
    }

    /// Before writing: a changed element gets a new version, as Excalidraw
    /// does, so other copies of the drawing see it as newer.
    pub fn stamp(&mut self) {
        if !self.touched {
            return;
        }
        let v = self.get("version").as_i64().unwrap_or(0) + 1;
        self.v.insert("version".into(), json!(v));
        self.v.insert("versionNonce".into(), json!((random_u64() % 2_000_000_000) as i64));
        self.v.insert("updated".into(), json!(now_ms()));
        self.touched = false;
    }
}

pub fn rotate(p: (f32, f32), c: (f32, f32), a: f32) -> (f32, f32) {
    if a == 0.0 {
        return p;
    }
    let (s, co) = a.sin_cos();
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    (c.0 + dx * co - dy * s, c.1 + dx * s + dy * co)
}

pub fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

pub fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0);
    dist(p, (a.0 + t * dx, a.1 + t * dy))
}

/// Point in polygon (even-odd).
pub fn inside(poly: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
        j = i;
    }
    c
}

/// Fractional indices (Excalidraw's `index`): the n-th key of an evenly
/// spaced run, "a0", "a1"… then two-digit keys.
pub fn nth_index(n: usize) -> String {
    const D: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if n < 62 {
        return format!("a{}", D[n] as char);
    }
    let n = n - 62;
    if n < 62 * 62 {
        return format!("b{}{}", D[n / 62] as char, D[n % 62] as char);
    }
    let n = n - 62 * 62;
    format!("c{}{}{}", D[(n / 3844) % 62] as char, D[(n / 62) % 62] as char, D[n % 62] as char)
}

/// Give elements `index` keys in their order when they have them and the
/// order no longer matches (after moving one to the front, adding one…).
pub fn fix_indices(els: &mut [Element]) {
    if !els.iter().any(|e| e.v.contains_key("index")) {
        return;
    }
    let keys: Vec<Option<&str>> = els.iter().map(|e| e.get("index").as_str()).collect();
    let ordered = keys.iter().all(|k| k.is_some()) && keys.windows(2).all(|w| w[0] < w[1]);
    if ordered {
        return;
    }
    for (i, e) in els.iter_mut().enumerate() {
        e.v.insert("index".into(), json!(nth_index(i)));
        e.touched = true;
        e.rev += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_indices() {
        let id = new_id();
        assert_eq!(id.len(), 8);
        assert_ne!(new_id(), new_id());
        assert_eq!(nth_index(0), "a0");
        assert_eq!(nth_index(61), "az");
        assert!(nth_index(61) < nth_index(62));
        assert!(nth_index(62 + 3843) < nth_index(62 + 3844));
    }

    #[test]
    fn points_and_hits() {
        let mut a = Element::new("arrow", 10.0, 10.0, 100.0, 0.0, &Style::default());
        a.set_abs_points(&[(10.0, 10.0), (110.0, 60.0)]);
        assert_eq!(a.points(), vec![(0.0, 0.0), (100.0, 50.0)]);
        assert_eq!((a.w(), a.h()), (100.0, 50.0));
        assert!(a.hit(60.0, 35.0, 4.0));
        assert!(!a.hit(60.0, 0.0, 4.0));
        let r = Element::new("rectangle", 0.0, 0.0, 100.0, 50.0, &Style::default());
        assert!(r.hit(0.0, 25.0, 3.0));
        assert!(!r.hit(50.0, 25.0, 3.0), "an empty rectangle is grabbed by its outline");
    }
}
