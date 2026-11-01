// The only native boundary: a main-thread-owned, retained controller.
// No video frames cross the Rust FFI. AVKit/WebKit own decoding and clocks.
#import <AppKit/AppKit.h>
#import <AVKit/AVKit.h>
#import <WebKit/WebKit.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <stdbool.h>

@class ATWorkVideo;
@interface ATVideoMessages : NSObject <WKScriptMessageHandler>
@property(nonatomic, weak) ATWorkVideo *owner;
@end

@interface ATWorkVideo : NSObject <NSWindowDelegate, WKNavigationDelegate, WKUIDelegate>
@property(nonatomic, strong) NSPanel *panel;
@property(nonatomic, strong) NSView *surface;
@property(nonatomic, strong) AVPlayerView *localView;
@property(nonatomic, strong) AVPlayer *player;
@property(nonatomic, strong) WKWebView *web;
@property(nonatomic, strong) NSButton *pin;
@property(nonatomic, weak) NSView *host;
@property(nonatomic) NSInteger mode; // 0 hidden, 1 in the app, 2 explicitly detached
@property(nonatomic) BOOL pinned;
@property(nonatomic) BOOL ready;
@property(nonatomic) BOOL wantsPlayback;
@property(nonatomic) BOOL suspended;
@property(nonatomic) BOOL buffering;
@property(nonatomic) BOOL stateChanged;
@property(nonatomic) NSUInteger commandSerial;
@property(nonatomic, copy) NSString *presentationRequested;
@property(nonatomic, copy) NSString *page;
@property(nonatomic, copy) NSString *title;
@property(nonatomic, copy) NSString *error;
@property(nonatomic, copy) NSString *external;
@property(nonatomic, copy) NSString *origin;
@property(nonatomic, copy) NSString *videoID;
@property(nonatomic) double position;
@property(nonatomic) BOOL playing;

@property(nonatomic) NSUInteger generation;
- (void)receive:(NSDictionary *)data;
- (void)stop;
- (void)pause;
@end

@implementation ATVideoMessages
- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message {
    // Subframes (including the remote YouTube frame) cannot write app state.
    if (message.webView == self.owner.web && message.frameInfo.isMainFrame && [message.body isKindOfClass:NSDictionary.class]) {
        [self.owner receive:message.body];
    }
}
@end

@implementation ATWorkVideo
- (instancetype)init {
    if (!(self = [super init])) return nil;
    _title = @"Vídeo"; _error = @""; _external = @""; _videoID = @"";
    _presentationRequested = @""; _pinned = YES;
    // No window is created until the user explicitly detaches the video.
    _surface = [[NSView alloc] initWithFrame:NSZeroRect];
    _surface.wantsLayer = YES;
    _surface.layer.backgroundColor = NSColor.blackColor.CGColor;
    _surface.layer.masksToBounds = YES;
    _surface.hidden = YES; _suspended = YES;
    return self;
}
- (BOOL)presented {
    NSWindow *window = self.surface.window;
    return !self.surface.isHiddenOrHasHiddenAncestor && window.isVisible && !window.isMiniaturized;
}
- (void)playWhenVisible {
    if (!self.wantsPlayback || !self.ready || !self.presented) return;
    [self.player play];

}
- (void)makePanel {
    if (self.panel) return;
    self.panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 480, 302)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
        backing:NSBackingStoreBuffered defer:NO];
    self.panel.title = [self.title stringByAppendingString:@" — Ateliet"];
    self.panel.releasedWhenClosed = NO;
    self.panel.hidesOnDeactivate = NO;
    self.panel.becomesKeyOnlyIfNeeded = YES;
    self.panel.delegate = self;
    self.panel.collectionBehavior = NSWindowCollectionBehaviorFullScreenAuxiliary;
    self.panel.contentMinSize = NSMakeSize(360, 232);
    [self.panel setFrameAutosaveName:@"AtelietWorkVideo"];
    NSRect screen = (self.host.window.screen ?: NSScreen.mainScreen).visibleFrame;
    NSRect frame = self.panel.frame;
    frame.origin = NSMakePoint(NSMaxX(screen) - frame.size.width - 24, NSMinY(screen) + 40);
    if (![self.panel setFrameUsingName:@"AtelietWorkVideo"]) [self.panel setFrame:frame display:NO];
    NSStackView *tools = [[NSStackView alloc] initWithFrame:NSZeroRect];
    tools.translatesAutoresizingMaskIntoConstraints = NO;
    tools.orientation = NSUserInterfaceLayoutOrientationHorizontal;
    tools.spacing = 8; tools.edgeInsets = NSEdgeInsetsMake(2, 8, 2, 8);
    self.pin = [NSButton checkboxWithTitle:@"Sempre por cima" target:self action:@selector(togglePin:)];
    self.pin.state = self.pinned ? NSControlStateValueOn : NSControlStateValueOff;
    [tools addArrangedSubview:[NSButton buttonWithTitle:@"Voltar ao Ateliet" target:self action:@selector(returnToApp:)]];
    [tools addArrangedSubview:self.pin];
    [tools addArrangedSubview:[NSButton buttonWithTitle:@"Abrir fora" target:self action:@selector(openExternal:)]];
    NSView *content = self.panel.contentView;
    [content addSubview:tools];
    [NSLayoutConstraint activateConstraints:@[
        [tools.topAnchor constraintEqualToAnchor:content.topAnchor],
        [tools.leadingAnchor constraintEqualToAnchor:content.leadingAnchor],
        [tools.trailingAnchor constraintEqualToAnchor:content.trailingAnchor],
        [tools.heightAnchor constraintEqualToConstant:32]
    ]];
    [self togglePin:nil];
}
- (void)setNotice:(NSString *)message {
    if (![self.error isEqualToString:message ?: @""]) self.stateChanged = YES;
    self.error = message ?: @"";
    self.panel.subtitle = self.error;
}
- (void)togglePin:(id)sender {
    self.pinned = self.pin.state == NSControlStateValueOn;
    self.panel.level = self.pinned ? NSFloatingWindowLevel : NSNormalWindowLevel;
}
- (void)returnToApp:(id)sender {
    self.presentationRequested = @"docked";
    // Rust reserves the space on the next layout pass before moving the view.
    [self.host.window makeKeyAndOrderFront:nil];
}
- (void)syncWebState {
    if (!self.web) return;
    NSString *js = [NSString stringWithFormat:@"window.atelietSetState && window.atelietSetState(%lu, %@, %@)",
        (unsigned long)self.commandSerial, self.wantsPlayback ? @"true" : @"false", self.presented ? @"true" : @"false"];
    [self.web evaluateJavaScript:js completionHandler:nil];
}
- (void)setSurfaceHidden:(BOOL)hidden {
    if (hidden == self.surface.isHidden) return;
    // Capture the user's intent before suspending. A waiting player has rate
    // zero too, but still intends to play; it must not become a manual pause.
    if (hidden) [self capturePosition];
    self.suspended = hidden;
    self.surface.hidden = hidden;
    self.commandSerial++; self.stateChanged = YES;
    if (hidden) { [self.player pause]; self.playing = NO; }
    else [self playWhenVisible];
    [self syncWebState];
}
- (void)placeIn:(NSView *)host frame:(NSRect)frame mode:(NSInteger)mode {
    self.host = host;
    // Do not reopen a panel closed by the user while Rust awaits the event.
    if (self.presentationRequested.length) return;
    if (mode == 2) {
        [self makePanel];
        if (self.surface.superview != self.panel.contentView) {
            [self setSurfaceHidden:YES];
            [self.surface removeFromSuperview];
            self.surface.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
            NSRect bounds = self.panel.contentView.bounds;
            self.surface.frame = NSMakeRect(0, 0, bounds.size.width, MAX(0, bounds.size.height - 32));
            [self.panel.contentView addSubview:self.surface positioned:NSWindowBelow relativeTo:nil];
        }
        if (self.mode != 2) { [self.panel deminiaturize:nil]; [self.panel orderFront:nil]; }
        [self setSurfaceHidden:self.panel.isMiniaturized];
    } else {
        if (self.surface.superview != host) [self setSurfaceHidden:YES];
        if (self.panel.isVisible) [self.panel orderOut:nil];
        if (mode != 1) {
            [self setSurfaceHidden:YES]; self.mode = mode;
            return; // Never resize a suspended WebKit/AVKit view to zero.
        }
        if (host && self.surface.superview != host) {
            [self.surface removeFromSuperview];
            self.surface.autoresizingMask = NSViewNotSizable;
            [host addSubview:self.surface positioned:NSWindowAbove relativeTo:nil];
        }
        if (!NSEqualRects(frame, self.surface.frame)) self.surface.frame = frame;
        BOOL fits = frame.size.width >= 199.5 && frame.size.height >= 199.5 && NSContainsRect(host.bounds, frame);
        [self setSurfaceHidden:mode != 1 || !host || !fits || !host.window.isVisible || host.window.isMiniaturized];
    }
    self.mode = mode;
}
- (void)openExternal:(id)sender {
    NSURL *url = [NSURL URLWithString:self.external];
    if (url.isFileURL || [@[@"https", @"http"] containsObject:url.scheme]) {
        [self pause];
        [NSWorkspace.sharedWorkspace openURL:url];
    }
}
- (void)capturePosition {
    if (self.player) {
        double t = CMTimeGetSeconds(self.player.currentTime);
        if (isfinite(t)) self.position = MAX(0, t);
        double duration = CMTimeGetSeconds(self.player.currentItem.duration);
        if (isfinite(duration) && duration > 0 && self.position >= duration - 0.25) self.position = 0;
        self.playing = self.player.timeControlStatus == AVPlayerTimeControlStatusPlaying;
        self.buffering = self.player.timeControlStatus == AVPlayerTimeControlStatusWaitingToPlayAtSpecifiedRate;
        if (self.ready && !self.suspended && self.presented) {
            if (self.playing) self.wantsPlayback = YES;
            else if (!self.buffering) self.wantsPlayback = NO;
        }
        if (self.player.currentItem.status == AVPlayerItemStatusFailed) {
            [self setNotice:@"Não foi possível ler este vídeo. Tente outro formato ou abra no app padrão."];
        }
    }
}
- (void)pause {
    [self capturePosition];
    self.wantsPlayback = NO; self.playing = NO;
    self.commandSerial++; self.stateChanged = YES;
    [self.player pause]; [self syncWebState];
}
- (void)requestPlay {
    self.wantsPlayback = YES;
    self.commandSerial++; self.stateChanged = YES;
    [self playWhenVisible]; [self syncWebState];
}
- (void)stop {
    [self pause];
    self.generation++; self.ready = NO; self.buffering = NO;
    [self.web.configuration.userContentController removeScriptMessageHandlerForName:@"video"];
    self.web.navigationDelegate = nil; self.web.UIDelegate = nil;
    [self.web stopLoading]; [self.web removeFromSuperview]; self.web = nil;
    self.localView.player = nil; [self.localView removeFromSuperview]; self.localView = nil;
    [self.player replaceCurrentItemWithPlayerItem:nil]; self.player = nil;
}
- (BOOL)windowShouldClose:(NSWindow *)sender {
    [self pause]; [self setSurfaceHidden:YES];
    self.presentationRequested = @"collapsed";
    return YES;
}
- (void)windowWillMiniaturize:(NSNotification *)note { [self pause]; }
- (void)windowDidDeminiaturize:(NSNotification *)note { [self setSurfaceHidden:NO]; }
- (void)receive:(NSDictionary *)data {
    if ([data[@"ready"] boolValue]) {
        self.ready = YES; self.stateChanged = YES;
        // Commands issued before the wrapper or the iframe existed converge
        // here. The wrapper starts invisible and never autoplays on its own.
        [self syncWebState];
    }
    if ([data[@"error"] isKindOfClass:NSString.class]) [self setNotice:data[@"error"]];
    if ([data[@"serial"] unsignedIntegerValue] != self.commandSerial) return;
    if ([data[@"id"] isKindOfClass:NSString.class]) {
        NSString *id = data[@"id"];
        NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:@"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-"];
        if (id.length == 11 && [id rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound) {
            self.videoID = id;
            self.external = [@"https://www.youtube.com/watch?v=" stringByAppendingString:id];
        }
    }
    if ([data[@"position"] isKindOfClass:NSNumber.class]) {
        double t = [data[@"position"] doubleValue];
        if (isfinite(t)) self.position = MAX(0, t);
    }
    if ([data[@"playing"] isKindOfClass:NSNumber.class]) {
        BOOL playing = [data[@"playing"] boolValue];
        if (self.playing != playing) self.stateChanged = YES;
        self.playing = playing;
        self.buffering = [data[@"buffering"] boolValue];
        if ([data[@"intent"] isKindOfClass:NSNumber.class]) self.wantsPlayback = [data[@"intent"] boolValue];
        if (self.playing) [self setNotice:@""];
        if (!self.presented && self.playing) [self syncWebState];
    }
    if ([data[@"title"] isKindOfClass:NSString.class]) {
        NSString *title = data[@"title"];
        NSRange range = [title rangeOfComposedCharacterSequencesForRange:NSMakeRange(0, MIN(200, title.length))];
        self.title = [title substringWithRange:range];
        self.panel.title = [self.title stringByAppendingString:@" — Ateliet"];
    }
    if ([data[@"error"] isKindOfClass:NSString.class]) [self setNotice:data[@"error"]];
}
- (void)mount:(NSView *)view {
    view.frame = self.surface.bounds;
    view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    [self.surface addSubview:view];
}
- (void)load:(NSDictionary *)source {
    [self stop];
    self.position = MAX(0, [source[@"position"] doubleValue]);
    [self setNotice:@""];
    self.title = source[@"title"] ?: @"Vídeo";
    self.panel.title = [self.title stringByAppendingString:@" — Ateliet"];
    self.pinned = [source[@"pinned"] boolValue];
    if (self.pin) { self.pin.state = self.pinned ? NSControlStateValueOn : NSControlStateValueOff; [self togglePin:nil]; }
    self.wantsPlayback = YES; self.commandSerial++;
    self.stateChanged = YES;
    NSDictionary *media = source[@"source"];
    if ([media[@"kind"] isEqual:@"local"]) {
        self.videoID = @"";
        NSURL *url = [NSURL fileURLWithPath:media[@"path"]];
        self.external = url.absoluteString;
        self.player = [AVPlayer playerWithURL:url];
        self.localView = [[AVPlayerView alloc] initWithFrame:self.surface.bounds];
        self.localView.controlsStyle = AVPlayerViewControlsStyleFloating;
        self.localView.videoGravity = AVLayerVideoGravityResizeAspect;
        self.localView.allowsPictureInPicturePlayback = YES;
        self.localView.player = self.player;
        [self mount:self.localView];
        NSUInteger generation = self.generation;
        __weak ATWorkVideo *weak = self;
        [self.player seekToTime:CMTimeMakeWithSeconds(self.position, 600) completionHandler:^(BOOL finished) {
            dispatch_async(dispatch_get_main_queue(), ^{
                ATWorkVideo *owner = weak;
                if (finished && owner.generation == generation) { owner.ready = YES; [owner playWhenVisible]; }
            });
        }];
    } else {
        NSString *id = media[@"id"];
        self.videoID = id;
        self.external = [@"https://www.youtube.com/watch?v=" stringByAppendingString:id];
        WKWebViewConfiguration *configuration = [[WKWebViewConfiguration alloc] init];
        configuration.mediaTypesRequiringUserActionForPlayback = WKAudiovisualMediaTypeNone;
        ATVideoMessages *messages = [[ATVideoMessages alloc] init]; messages.owner = self;
        [configuration.userContentController addScriptMessageHandler:messages name:@"video"];
        self.web = [[WKWebView alloc] initWithFrame:self.surface.bounds configuration:configuration];
        self.web.navigationDelegate = self; self.web.UIDelegate = self;
        [self mount:self.web];
        NSString *bundle = NSBundle.mainBundle.bundleIdentifier ?: @"md.ateliet.app";
        NSString *origin = [[@"https://" stringByAppendingString:bundle] lowercaseString];
        self.origin = origin;
        NSDictionary *config = @{@"id":id, @"position":@(self.position), @"origin":origin};
        NSString *json = [[NSString alloc] initWithData:[NSJSONSerialization dataWithJSONObject:config options:0 error:nil] encoding:NSUTF8StringEncoding];
        NSString *html = [self.page stringByReplacingOccurrencesOfString:@"__ATELIET_CONFIG__" withString:json];
        [self.web loadHTMLString:html baseURL:[NSURL URLWithString:origin]];
    }
}
- (void)webView:(WKWebView *)webView decidePolicyForNavigationAction:(WKNavigationAction *)action decisionHandler:(void (^)(WKNavigationActionPolicy))handler {
    // The trusted wrapper stays in place. Remote links open outside the app.
    NSURL *url = action.request.URL;
    BOOL trusted = [url.absoluteString isEqual:@"about:blank"] ||
        ([url.scheme isEqual:@"https"] && [url.host isEqual:[NSURL URLWithString:self.origin].host]);
    if ((action.targetFrame.isMainFrame && !trusted) || !action.targetFrame) {
        if (action.navigationType == WKNavigationTypeLinkActivated && [@[@"https", @"http"] containsObject:url.scheme]) {
            [self pause]; [NSWorkspace.sharedWorkspace openURL:url];
        }
        handler(WKNavigationActionPolicyCancel); return;
    }
    handler(WKNavigationActionPolicyAllow);
}
- (WKWebView *)webView:(WKWebView *)webView createWebViewWithConfiguration:(WKWebViewConfiguration *)configuration forNavigationAction:(WKNavigationAction *)action windowFeatures:(WKWindowFeatures *)features {
    NSURL *url = action.request.URL;
    if ([@[@"https", @"http"] containsObject:url.scheme]) { [self pause]; [NSWorkspace.sharedWorkspace openURL:url]; }
    return nil;
}
- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation {
    [self syncWebState];
}
- (void)webView:(WKWebView *)webView didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {
    if (error.code != NSURLErrorCancelled) [self setNotice:@"Sem conexão com o YouTube. Verifique a rede ou abra no navegador."];
}
- (void)webViewWebContentProcessDidTerminate:(WKWebView *)webView {
    self.playing = NO; self.ready = NO;
    [self setNotice:@"A conexão do player foi interrompida. Tente novamente para retomar daqui."];
}
@end

// Every entry point must run on the application's main thread.
void *ateliet_video_create(const char *html) {
    NSCAssert(NSThread.isMainThread, @"video must run on the main thread");
    ATWorkVideo *video = [[ATWorkVideo alloc] init];
    video.page = [NSString stringWithUTF8String:html];
    return (__bridge_retained void *)video;
}
void ateliet_video_open(void *handle, const char *json) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    NSData *data = [[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *source = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if ([source isKindOfClass:NSDictionary.class]) [video load:source];
}
void ateliet_video_toggle(void *handle) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    [video capturePosition];
    if (video.wantsPlayback || video.playing) [video pause];
    else [video requestPlay];
}
void ateliet_video_pause(void *handle) { [(__bridge ATWorkVideo *)handle pause]; }
void ateliet_video_play(void *handle) { [(__bridge ATWorkVideo *)handle requestPlay]; }
// Coordinates arrive in egui points, relative to its root viewport. Scaling
// against the actual host bounds handles Retina and egui zoom independently.
void ateliet_video_place(void *handle, void *view, double sw, double sh,
                         double x, double y, double w, double h, int mode) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    NSView *host = (__bridge NSView *)view;
    if (!host || sw <= 0 || sh <= 0) { [video setSurfaceHidden:YES]; return; }
    NSRect bounds = host.bounds;
    double sx = bounds.size.width / sw, sy = bounds.size.height / sh;
    double top = host.isFlipped ? NSMinY(bounds) + y * sy : NSMaxY(bounds) - (y + h) * sy;
    NSRect frame = NSMakeRect(NSMinX(bounds) + x * sx, top, w * sx, h * sy);
    [video placeIn:host frame:frame mode:mode];
}
bool ateliet_video_has_event(void *handle) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    return video.presentationRequested.length != 0 || video.stateChanged;
}
char *ateliet_video_snapshot(void *handle) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    [video capturePosition];
    NSDictionary *snapshot = @{@"position":@(video.position), @"playing":@(video.playing),
        @"visible":@(video.presented), @"pinned":@(video.pinned),
        @"ready":@(video.ready), @"buffering":@(video.buffering), @"suspended":@(video.suspended),
        @"intent":@(video.wantsPlayback),
        @"title":video.title, @"error":video.error, @"id":video.videoID, @"presentation":video.presentationRequested};
    video.presentationRequested = @""; video.stateChanged = NO;
    NSData *data = [NSJSONSerialization dataWithJSONObject:snapshot options:0 error:nil];
    if (!data) return NULL;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    return strdup(json.UTF8String);
}
void ateliet_video_free_string(char *value) { free(value); }
void ateliet_video_destroy(void *handle) {
    ATWorkVideo *video = (__bridge_transfer ATWorkVideo *)handle;
    [video stop]; [video.surface removeFromSuperview];
    video.panel.delegate = nil; [video.panel close];
}
