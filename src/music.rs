//! Music to write by: the vault's audio played in the app, the macOS Music
//! app driven through `osascript` (its playlists become a book's
//! soundtrack, with covers), links opened in their service. While a
//! chapter is being written, what plays goes into the book's diary.
//! Music never changes on its own.

use ateliet_core::dataview::index::Index;
use ateliet_core::scriptorium::book::{self, Book};
use ateliet_core::scriptorium::trilha::{self as tr, AppTrack};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use eframe::egui;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Source {
    #[default]
    None,
    App,
    File,
}

/// What is playing.
#[derive(Clone, Debug, Default)]
pub struct Now {
    pub source: Source,
    pub playing: bool,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub key: String,
    pub pos: f64,
    pub dur: f64,
}

impl Now {
    pub fn some(&self) -> bool {
        self.source != Source::None && !self.name.is_empty()
    }
}

/// What the views ask of the music.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Play a track of a book's soundtrack (book path, key); with a chapter, note it as "written listening to".
    Play(String, String, Option<String>),
    /// Play a link or an audio file of the vault (source note for links).
    PlaySrc(String, String),
    PlayPause,
    Next,
    Previous,
    /// Read the Music app's playlists to pick the book's.
    Connect(String),
    /// Bring the tracks of the book's playlist again.
    Refresh(String),
    PlayPlaylist(String),
    Reveal(String, String),
}

enum Job {
    Now(Option<Now>),
    Playlists(String, Result<Vec<(String, String)>, String>),
    Imported(Result<String, String>),
    Progress(String),
    Error(String),
}

pub struct Music {
    #[cfg(feature = "audio")]
    stream: Option<rodio::OutputStream>,
    #[cfg(feature = "audio")]
    sink: Option<rodio::Sink>,
    file_meta: Option<(String, String, f64)>,
    pub now: Now,
    app_now: Option<Now>,
    tx: Sender<Job>,
    rx: Receiver<Job>,
    polling: bool,
    last_poll: Instant,
    last_tick: Instant,
    last_flush: Instant,
    pub last_edit: Option<Instant>,
    pub last_chapter: Option<String>,
    /// book → ({chapter: {key: secs}}, {key: meta})
    acc: HashMap<String, (Json, Json)>,
    /// Whether the Music app answered once (the plugin's `musicaApp`).
    pub app_ok: bool,
    pub diary: bool,
    pub busy: Option<String>,
    pub toasts: Vec<String>,
    /// Playlists read from the app, to pick one for a book.
    pub playlists: Option<(String, Vec<(String, String)>)>,
    /// A soundtrack file the app wrote (to refresh the index).
    pub written: Vec<String>,
}

impl Default for Music {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Music {
            #[cfg(feature = "audio")]
            stream: None,
            #[cfg(feature = "audio")]
            sink: None,
            file_meta: None,
            now: Now::default(),
            app_now: None,
            tx,
            rx,
            polling: false,
            last_poll: Instant::now() - Duration::from_secs(60),
            last_tick: Instant::now(),
            last_flush: Instant::now(),
            last_edit: None,
            last_chapter: None,
            acc: HashMap::new(),
            app_ok: false,
            diary: true,
            busy: None,
            toasts: Vec::new(),
            playlists: None,
            written: Vec::new(),
        }
    }
}

pub fn mac() -> bool {
    cfg!(target_os = "macos")
}

// ── the Music app (JXA / AppleScript, as in the plugin) ──────────────────

fn osascript(src: &str, lang: &str) -> Result<String, String> {
    let out = std::process::Command::new("osascript").args(["-l", lang, "-e", src]).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let e = String::from_utf8_lossy(&out.stderr).to_string();
        if e.contains("-1743") || e.to_lowercase().contains("not authori") {
            Err("O macOS não deixou o Ateliet controlar o app Música. Abra Ajustes do Sistema › Privacidade e Segurança › Automação e ative \"Música\" para o Ateliet.".into())
        } else {
            Err(e.trim().chars().take(200).collect())
        }
    }
}

fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn app_playlists() -> Result<Vec<(String, String)>, String> {
    let out = osascript(
        "(() => { const M = Application('Music'); const ps = M.userPlaylists; const n = ps.name(), id = ps.persistentID(), sk = ps.specialKind(); return JSON.stringify(n.map((x, i) => ({ name: x, id: id[i], special: String(sk[i]) }))); })()",
        "JavaScript",
    )?;
    let v: Vec<Json> = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(v.into_iter().filter(|p| p["special"] == "none").map(|p| (p["id"].as_str().unwrap_or("").to_string(), p["name"].as_str().unwrap_or("").to_string())).collect())
}

fn app_tracks(pid: &str) -> Result<Vec<AppTrack>, String> {
    let out = osascript(&format!("(() => {{ const M = Application('Music'); const p = M.userPlaylists.whose({{ persistentID: {} }})[0]; const t = p.tracks; const ids = t.persistentID(), names = t.name(), artists = t.artist(), albums = t.album(), durs = t.duration(), years = t.year(), genres = t.genre(); return JSON.stringify(ids.map((x, i) => ({{ id: x, name: names[i], artist: artists[i], album: albums[i], dur: durs[i], year: years[i], genre: genres[i] }}))); }})()", q(pid)), "JavaScript")?;
    serde_json::from_str(&out).map_err(|e| e.to_string())
}

fn app_now() -> Option<Now> {
    let out = osascript(
        "(() => { const M = Application('Music'); if (!M.running()) return JSON.stringify({ state: 'off' }); const st = String(M.playerState()); let t = null, pos = 0; try { const c = M.currentTrack; t = { name: c.name(), artist: c.artist(), album: c.album(), dur: c.duration() }; } catch (e) {} try { pos = M.playerPosition(); } catch (e) {} return JSON.stringify({ state: st, track: t, pos }); })()",
        "JavaScript",
    )
    .ok()?;
    let v: Json = serde_json::from_str(&out).ok()?;
    let t = &v["track"];
    if !t.is_object() {
        return None;
    }
    let (name, artist) = (t["name"].as_str().unwrap_or("").to_string(), t["artist"].as_str().unwrap_or("").to_string());
    Some(Now { source: Source::App, playing: v["state"] == "playing", key: tr::track_key(&artist, &name), name, artist, album: t["album"].as_str().unwrap_or("").into(), pos: v["pos"].as_f64().unwrap_or(0.0), dur: t["dur"].as_f64().unwrap_or(0.0) })
}

fn app_running() -> bool {
    osascript("Application('Music').running()", "JavaScript").is_ok_and(|s| s == "true")
}

fn app_cmd(c: &str) -> Result<String, String> {
    osascript(&format!("tell application \"Music\" to {c}"), "AppleScript")
}

fn app_play_track(pid: &str, tid: &str) -> Result<String, String> {
    osascript(&format!("tell application \"Music\"\nset p to (first user playlist whose persistent ID is {})\nplay (first track of p whose persistent ID is {})\nend tell", q(pid), q(tid)), "AppleScript")
}

fn app_artwork(pid: &str, tid: &str) -> Option<Vec<u8>> {
    let tmp = std::env::temp_dir().join(format!("ateliet-art-{}", tr::uni_slug(tid)));
    let script = format!(
        "set outPath to POSIX file {}\ntell application \"Music\"\nset p to (first user playlist whose persistent ID is {})\nset t to (first track of p whose persistent ID is {})\nif (count of artworks of t) is 0 then return \"none\"\nset d to raw data of artwork 1 of t\nend tell\nset f to open for access outPath with write permission\nset eof of f to 0\nwrite d to f\nclose access f\nreturn \"ok\"",
        q(&tmp.to_string_lossy()),
        q(pid),
        q(tid)
    );
    let ok = osascript(&script, "AppleScript").ok()? == "ok";
    let bytes = std::fs::read(&tmp).ok();
    let _ = std::fs::remove_file(&tmp);
    if ok {
        bytes
    } else {
        None
    }
}

/// A small cover (≤ 320 px, JPEG) in `<book>/Trilha/capas/`.
fn save_cover(root: &Path, b: &Book, key: &str, bytes: &[u8]) -> Option<String> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > 320 || img.height() > 320 { img.thumbnail(320, 320) } else { img };
    let slug: String = tr::uni_slug(key).chars().take(80).collect();
    let rel = format!("{}/Trilha/capas/{slug}.jpg", b.folder);
    let abs = root.join(&rel);
    std::fs::create_dir_all(abs.parent()?).ok()?;
    img.to_rgb8().save_with_format(&abs, image::ImageFormat::Jpeg).ok()?;
    Some(rel)
}

fn import(root: &Path, ix_book: (Book, String), pid: &str, pname: &str, tx: &Sender<Job>) -> Result<String, String> {
    let (b, link) = ix_book;
    let tracks = app_tracks(pid)?;
    let mut data = tr::load(root, &b);
    let today = ateliet_core::datefmt::today().format("%Y-%m-%d").to_string();
    let (need, rep, fora) = tr::merge_playlist(&mut data, pid, pname, &tracks, &today);
    for (n, i) in need.iter().enumerate() {
        let _ = tx.send(Job::Progress(format!("Importando \"{pname}\"… capas {}/{}", n + 1, need.len())));
        let t = data["faixas"][*i].clone();
        let (tid, key) = (t["app"]["tid"].as_str().unwrap_or("").to_string(), tr::str_of(&t, "k").to_string());
        if let Some(rel) = app_artwork(pid, &tid).and_then(|bytes| save_cover(root, &b, &key, &bytes)) {
            data["faixas"][*i]["capa"] = json!(rel);
        }
    }
    let p = tr::path(&b);
    let text = match std::fs::read_to_string(root.join(&p)) {
        Ok(t) => ateliet_core::scriptorium::write_json_block(&t, tr::LANG, &data),
        Err(_) => ateliet_core::scriptorium::write_json_block(
            &format!("---\nscriptorium: trilha\nlivro: \"{link}\"\n---\n\nTrilha sonora de **{}**: faixas, temas e o diário do que tocava enquanto você escrevia. Abra pela tela Trilha do Scriptorium.\n", b.title),
            tr::LANG,
            &data,
        ),
    };
    book::write(root, &p, &text)?;
    let n = data["faixas"].as_array().map(|a| a.iter().filter(|t| t["app"].is_object()).count()).unwrap_or(0);
    let mut msg = format!("{n} faixas de \"{pname}\" na trilha de {}.", b.title);
    if rep > 0 {
        msg += &format!(" {rep} {} na playlist.", if rep == 1 { "repetida ignorada" } else { "repetidas ignoradas" });
    }
    if fora > 0 {
        msg += &format!(" {fora} que você removeu {} fora.", if fora == 1 { "continua" } else { "continuam" });
    }
    let _ = tx.send(Job::Progress(msg));
    Ok(p)
}

impl Music {
    fn spawn(&self, f: impl FnOnce(&Sender<Job>) + Send + 'static) {
        let tx = self.tx.clone();
        std::thread::spawn(move || f(&tx));
    }

    /// Every frame: results of the background work, the poll of what plays, the diary.
    pub fn tick(&mut self, ctx: &egui::Context, root: &Path, ix: &Index, author: &str) {
        while let Ok(j) = self.rx.try_recv() {
            match j {
                Job::Now(n) => {
                    self.polling = false;
                    self.app_now = n;
                }
                Job::Playlists(b, Ok(list)) => {
                    self.busy = None;
                    self.app_ok = true;
                    self.playlists = Some((b, list));
                }
                Job::Playlists(_, Err(e)) | Job::Imported(Err(e)) | Job::Error(e) => {
                    self.busy = None;
                    self.toasts.push(format!("App Música: {e}"));
                }
                Job::Imported(Ok(p)) => {
                    self.busy = None;
                    self.written.push(p);
                }
                Job::Progress(m) => {
                    if m.starts_with("Importando") {
                        self.busy = Some(m);
                    } else {
                        self.toasts.push(m);
                    }
                }
            }
        }
        // the app is asked only while someone looks (a soundtrack open) or writes
        let writing = self.last_edit.is_some_and(|t| t.elapsed() < Duration::from_secs(60));
        if mac() && self.app_ok && !self.polling && self.last_poll.elapsed() > Duration::from_secs(5) && (writing || self.app_now.is_some()) {
            self.polling = true;
            self.last_poll = Instant::now();
            self.spawn(|tx| {
                let n = if app_running() { app_now() } else { None };
                let _ = tx.send(Job::Now(n));
            });
        }
        self.now = self.compose();
        if self.now.playing {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        // the diary: time with music while a chapter was being written
        let dt = self.last_tick.elapsed().as_secs_f64().min(10.0);
        if dt >= 1.0 {
            self.last_tick = Instant::now();
            if self.diary && self.now.playing && self.last_edit.is_some_and(|t| t.elapsed() < Duration::from_secs(45)) {
                if let Some(ch) = self.last_chapter.clone() {
                    if let Some(b) = book::book_for(ix, &ch, author) {
                        let e = self.acc.entry(b.path.clone()).or_insert((json!({}), json!({})));
                        let n = e.0[&ch][&self.now.key].as_f64().unwrap_or(0.0) + dt;
                        e.0[&ch][&self.now.key] = json!(n);
                        e.1[&self.now.key] = json!({ "nome": self.now.name, "artista": self.now.artist, "album": self.now.album });
                    }
                }
            }
        }
        if self.last_flush.elapsed() > Duration::from_secs(180) {
            self.flush(root, ix, author);
        }
    }

    fn compose(&mut self) -> Now {
        if let Some(n) = self.app_now.as_ref().filter(|n| n.playing) {
            return n.clone();
        }
        #[cfg(feature = "audio")]
        if let (Some(s), Some((name, artist, dur))) = (&self.sink, &self.file_meta) {
            if !s.empty() {
                return Now { source: Source::File, playing: !s.is_paused(), key: tr::track_key(artist, name), name: name.clone(), artist: artist.clone(), album: String::new(), pos: s.get_pos().as_secs_f64(), dur: *dur };
            }
        }
        self.app_now.clone().unwrap_or_default()
    }

    /// Write the diary into the soundtracks.
    pub fn flush(&mut self, root: &Path, ix: &Index, author: &str) {
        self.last_flush = Instant::now();
        for (bp, (chapters, metas)) in std::mem::take(&mut self.acc) {
            let Some(b) = book::book_by_path(ix, &bp, author) else { continue };
            let mut data = tr::load(root, &b);
            tr::add_diary(&mut data, &chapters, &metas);
            if let Ok(p) = tr::save(root, ix, &b, &data) {
                self.written.push(p);
            }
        }
    }

    /// The diary not written yet, for a book.
    pub fn pending(&self, book: &str) -> Option<&Json> {
        self.acc.get(book).map(|x| &x.0)
    }

    #[cfg(feature = "audio")]
    fn play_file(&mut self, abs: &Path, name: &str, artist: &str) -> Result<(), String> {
        if self.stream.is_none() {
            let mut s = rodio::OutputStreamBuilder::open_default_stream().map_err(|e| format!("sem saída de áudio ({e})"))?;
            s.log_on_drop(false);
            self.stream = Some(s);
        }
        let f = std::fs::File::open(abs).map_err(|e| e.to_string())?;
        let dec = rodio::Decoder::try_from(f).map_err(|e| format!("não consegui ler o áudio ({e})"))?;
        use rodio::Source as _;
        let dur = dec.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let sink = rodio::Sink::connect_new(self.stream.as_ref().unwrap().mixer());
        sink.append(dec);
        self.sink = Some(sink);
        self.file_meta = Some((name.to_string(), artist.to_string(), dur));
        // the app's player stops being "what plays"
        self.app_now = None;
        Ok(())
    }

    #[cfg(not(feature = "audio"))]
    fn play_file(&mut self, abs: &Path, _name: &str, _artist: &str) -> Result<(), String> {
        open::that(abs).map_err(|e| e.to_string())
    }

    /// Carry out what a view asked.
    pub fn run(&mut self, cmd: Cmd, root: &Path, ix: &Index, author: &str, resolve: &dyn Fn(&str, &str) -> Option<String>) {
        match cmd {
            Cmd::Play(bp, key, chapter) => {
                let Some(b) = book::book_by_path(ix, &bp, author) else { return };
                let mut data = tr::load(root, &b);
                let Some(i) = tr::find(&data, &key) else { return };
                let t = data["faixas"][i].clone();
                let r = if let (true, Some(pid), Some(tid)) = (mac() && self.app_ok, t["app"]["pid"].as_str(), t["app"]["tid"].as_str()) {
                    let (pid, tid) = (pid.to_string(), tid.to_string());
                    self.stop_file();
                    self.spawn(move |tx| {
                        if let Err(e) = app_play_track(&pid, &tid) {
                            let _ = tx.send(Job::Error(e));
                        }
                    });
                    self.last_poll = Instant::now() - Duration::from_secs(4);
                    self.app_now = Some(Now { source: Source::App, playing: true, name: tr::str_of(&t, "nome").into(), artist: tr::str_of(&t, "artista").into(), album: tr::str_of(&t, "album").into(), key: key.clone(), pos: 0.0, dur: t["dur"].as_f64().unwrap_or(0.0) });
                    Ok(())
                } else if !tr::str_of(&t, "arquivo").is_empty() {
                    match resolve(tr::str_of(&t, "arquivo"), &tr::path(&b)) {
                        Some(p) => self.play_file(&root.join(p), tr::str_of(&t, "nome"), tr::str_of(&t, "artista")),
                        None => Err(format!("Áudio não encontrado: {}", tr::str_of(&t, "arquivo"))),
                    }
                } else if !tr::str_of(&t, "link").is_empty() {
                    open::that(tr::str_of(&t, "link")).map_err(|e| e.to_string())
                } else {
                    let term = format!("{} {}", tr::str_of(&t, "nome"), tr::str_of(&t, "artista"));
                    open::that(format!("https://music.apple.com/search?term={}", percent(&term))).map_err(|e| e.to_string())
                };
                if let Err(e) = r {
                    self.toasts.push(format!("Não consegui tocar: {e}"));
                }
                if let Some(ch) = chapter {
                    data["inspiracao"][&ch] = json!({ "k": key, "quando": ateliet_core::datefmt::today().format("%Y-%m-%d").to_string() });
                    if let Ok(p) = tr::save(root, ix, &b, &data) {
                        self.written.push(p);
                    }
                }
            }
            Cmd::PlaySrc(src, source) => {
                let target = src.trim().trim_start_matches('!').trim_start_matches("[[").trim_end_matches("]]").split('|').next().unwrap_or("").to_string();
                let r = if src.starts_with("http") {
                    open::that(&src).map_err(|e| e.to_string())
                } else {
                    match resolve(&target, &source) {
                        Some(p) => {
                            let name = p.rsplit('/').next().unwrap_or(&p).rsplit_once('.').map(|x| x.0.to_string()).unwrap_or_default();
                            self.play_file(&root.join(&p), &name, "")
                        }
                        None => Err(format!("Áudio não encontrado: {target}")),
                    }
                };
                if let Err(e) = r {
                    self.toasts.push(format!("Não consegui tocar: {e}"));
                }
            }
            Cmd::PlayPause | Cmd::Next | Cmd::Previous => {
                let src = self.now.source;
                match src {
                    Source::App => {
                        let c = match cmd {
                            Cmd::PlayPause => "playpause",
                            Cmd::Next => "next track",
                            _ => "previous track",
                        };
                        if let Some(n) = self.app_now.as_mut() {
                            if matches!(cmd, Cmd::PlayPause) {
                                n.playing = !n.playing;
                            }
                        }
                        self.spawn(move |tx| {
                            if let Err(e) = app_cmd(c) {
                                let _ = tx.send(Job::Error(e));
                            }
                        });
                        self.last_poll = Instant::now() - Duration::from_secs(4);
                    }
                    Source::File => {
                        #[cfg(feature = "audio")]
                        if let Some(s) = &self.sink {
                            match cmd {
                                Cmd::PlayPause if s.is_paused() => s.play(),
                                Cmd::PlayPause => s.pause(),
                                Cmd::Previous => {
                                    let _ = s.try_seek(Duration::ZERO);
                                }
                                _ => self.stop_file(),
                            }
                        }
                    }
                    Source::None => {}
                }
            }
            Cmd::Connect(bp) => {
                if !mac() {
                    self.toasts.push("O app Música só pode ser controlado no Mac.".into());
                    return;
                }
                self.busy = Some("Lendo as playlists do app Música…".into());
                self.spawn(move |tx| {
                    let _ = tx.send(Job::Playlists(bp, app_playlists()));
                });
            }
            Cmd::Refresh(bp) => {
                let Some(b) = book::book_by_path(ix, &bp, author) else { return };
                let data = tr::load(root, &b);
                let (Some(pid), Some(name)) = (data["fonte"]["playlist"]["id"].as_str().map(String::from), data["fonte"]["playlist"]["nome"].as_str().map(String::from)) else { return };
                self.import(root, ix, &b, &pid, &name);
            }
            Cmd::PlayPlaylist(pid) => {
                self.stop_file();
                self.spawn(move |tx| {
                    if let Err(e) = osascript(&format!("tell application \"Music\" to play (first user playlist whose persistent ID is {})", q(&pid)), "AppleScript") {
                        let _ = tx.send(Job::Error(e));
                    }
                });
                self.last_poll = Instant::now() - Duration::from_secs(4);
                self.app_now = Some(Now { source: Source::App, playing: true, ..Default::default() });
            }
            Cmd::Reveal(pid, tid) => {
                self.spawn(move |tx| {
                    if let Err(e) = osascript(&format!("tell application \"Music\"\nset p to (first user playlist whose persistent ID is {})\nreveal (first track of p whose persistent ID is {})\nactivate\nend tell", q(&pid), q(&tid)), "AppleScript") {
                        let _ = tx.send(Job::Error(e));
                    }
                });
            }
        }
    }

    /// Bring a playlist of the app into a book's soundtrack (in the background).
    pub fn import(&mut self, root: &Path, ix: &Index, b: &Book, pid: &str, name: &str) {
        self.busy = Some(format!("Importando \"{name}\"…"));
        let (root, b2, pid, name) = (root.to_path_buf(), b.clone(), pid.to_string(), name.to_string());
        let link = book::note_link(ix, &b.path, &tr::path(b));
        self.spawn(move |tx| {
            let r = import(&root, (b2, link), &pid, &name, tx);
            let _ = tx.send(Job::Imported(r));
        });
    }

    fn stop_file(&mut self) {
        #[cfg(feature = "audio")]
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.file_meta = None;
    }
}

fn percent(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}
