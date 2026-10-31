// The only native boundary: a main-thread-owned, retained controller.
// No video frames cross the Rust FFI. AVKit/WebKit own decoding and clocks.
#import <AppKit/AppKit.h>
#import <AVKit/AVKit.h>
#import <WebKit/WebKit.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

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
@property(nonatomic, copy) NSString *page;
@property(nonatomic, copy) NSString *title;
@property(nonatomic, copy) NSString *error;
@property(nonatomic, copy) NSString *external;
@property(nonatomic, copy) NSString *origin;
@property(nonatomic, copy) NSString *videoID;
@property(nonatomic) double position;
@property(nonatomic) BOOL playing;
@property(nonatomic) BOOL changeRequested;
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
    _panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 480, 302)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
        backing:NSBackingStoreBuffered defer:NO];
    _panel.title = @"Vídeo — Ateliet";
    _panel.releasedWhenClosed = NO;
    _panel.hidesOnDeactivate = NO;
    _panel.becomesKeyOnlyIfNeeded = YES;
    _panel.delegate = self;
    _panel.collectionBehavior = NSWindowCollectionBehaviorFullScreenAuxiliary;
    _panel.contentMinSize = NSMakeSize(360, 232); // YouTube viewport >= 200 × 200.
    [_panel setFrameAutosaveName:@"AtelietWorkVideo"];
    NSRect screen = (NSApp.mainWindow.screen ?: NSScreen.mainScreen).visibleFrame;
    NSRect frame = _panel.frame;
    frame.origin = NSMakePoint(NSMaxX(screen) - frame.size.width - 24, NSMinY(screen) + 40);
    if (![_panel setFrameUsingName:@"AtelietWorkVideo"]) [_panel setFrame:frame display:NO];

    NSView *content = _panel.contentView;
    _surface = [[NSView alloc] initWithFrame:NSZeroRect];
    _surface.translatesAutoresizingMaskIntoConstraints = NO;
    _surface.wantsLayer = YES; _surface.layer.backgroundColor = NSColor.blackColor.CGColor;
    [content addSubview:_surface];
    NSStackView *tools = [[NSStackView alloc] initWithFrame:NSZeroRect];
    tools.translatesAutoresizingMaskIntoConstraints = NO;
    tools.orientation = NSUserInterfaceLayoutOrientationHorizontal;
    tools.spacing = 8; tools.edgeInsets = NSEdgeInsetsMake(2, 8, 2, 8);
    _pin = [NSButton checkboxWithTitle:@"Sempre por cima" target:self action:@selector(togglePin:)];
    _pin.toolTip = @"Manter o vídeo acima das outras janelas";
    NSButton *change = [NSButton buttonWithTitle:@"Trocar vídeo…" target:self action:@selector(changeVideo:)];
    NSButton *external = [NSButton buttonWithTitle:@"Abrir fora" target:self action:@selector(openExternal:)];
    external.toolTip = @"Abrir no navegador ou aplicativo padrão";
    [tools addArrangedSubview:_pin]; [tools addArrangedSubview:change]; [tools addArrangedSubview:external];
    [content addSubview:tools];
    // Errors use the native subtitle, never an overlay on the YouTube player.
    [NSLayoutConstraint activateConstraints:@[
        [tools.topAnchor constraintEqualToAnchor:content.topAnchor],
        [tools.leadingAnchor constraintEqualToAnchor:content.leadingAnchor],
        [tools.trailingAnchor constraintEqualToAnchor:content.trailingAnchor],
        [tools.heightAnchor constraintEqualToConstant:32],
        [_surface.topAnchor constraintEqualToAnchor:tools.bottomAnchor],
        [_surface.leadingAnchor constraintEqualToAnchor:content.leadingAnchor],
        [_surface.trailingAnchor constraintEqualToAnchor:content.trailingAnchor],
        [_surface.bottomAnchor constraintEqualToAnchor:content.bottomAnchor]
    ]];
    return self;
}
- (void)setNotice:(NSString *)message {
    self.error = message ?: @"";
    // Native subtitle keeps the video unobscured and the recovery action visible.
    self.panel.subtitle = self.error;
}
- (void)togglePin:(id)sender {
    self.panel.level = self.pin.state == NSControlStateValueOn ? NSFloatingWindowLevel : NSNormalWindowLevel;
}
- (void)changeVideo:(id)sender {
    self.changeRequested = YES;
    for (NSWindow *window in NSApp.windows) {
        if (window != self.panel && window.isVisible && ![window isKindOfClass:NSPanel.class]) {
            [window makeKeyAndOrderFront:nil]; break;
        }
    }
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
        self.playing = self.player.rate != 0;
        if (self.player.currentItem.status == AVPlayerItemStatusFailed) {
            [self setNotice:@"Não foi possível ler este vídeo. Tente outro formato ou abra no app padrão."];
        }
    }
}
- (void)pause {
    [self capturePosition];
    [self.player pause];
    [self.web evaluateJavaScript:@"window.atelietPause && window.atelietPause()" completionHandler:nil];
    self.playing = NO;
}
- (void)stop {
    [self pause];
    self.generation++;
    [self.web.configuration.userContentController removeScriptMessageHandlerForName:@"video"];
    self.web.navigationDelegate = nil; self.web.UIDelegate = nil;
    [self.web stopLoading]; [self.web removeFromSuperview]; self.web = nil;
    self.localView.player = nil; [self.localView removeFromSuperview]; self.localView = nil;
    [self.player replaceCurrentItemWithPlayerItem:nil]; self.player = nil;
}
- (BOOL)windowShouldClose:(NSWindow *)sender { [self stop]; return YES; }
- (void)windowWillMiniaturize:(NSNotification *)note { [self pause]; }
- (void)receive:(NSDictionary *)data {
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
        self.playing = [data[@"playing"] boolValue];
        if (self.playing) [self setNotice:@""];
        if (self.panel.isMiniaturized && self.playing) [self pause];
    }
    if ([data[@"title"] isKindOfClass:NSString.class]) {
        self.title = [data[@"title"] substringToIndex:MIN(200, [data[@"title"] length])];
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
    self.pin.state = [source[@"pinned"] boolValue] ? NSControlStateValueOn : NSControlStateValueOff;
    [self togglePin:nil];
    [self.panel deminiaturize:nil];
    [self.panel orderFront:nil]; // Opening media does not take typing focus.
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
                if (finished && owner.generation == generation && owner.panel.isVisible && !owner.panel.isMiniaturized) [owner.player play];
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
- (void)webView:(WKWebView *)webView didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {
    if (error.code != NSURLErrorCancelled) [self setNotice:@"Sem conexão com o YouTube. Verifique a rede ou abra no navegador."];
}
- (void)webViewWebContentProcessDidTerminate:(WKWebView *)webView {
    self.playing = NO;
    [self setNotice:@"O player foi interrompido. Reabra o vídeo pelos recentes."];
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
    if (video.player) { if (video.player.rate != 0) [video.player pause]; else [video.player play]; }
    else [video.web evaluateJavaScript:@"window.atelietToggle && window.atelietToggle()" completionHandler:nil];
}
void ateliet_video_reveal(void *handle) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    [video.panel deminiaturize:nil]; [video.panel orderFront:nil];
}
char *ateliet_video_snapshot(void *handle) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    [video capturePosition];
    NSDictionary *snapshot = @{@"position":@(video.position), @"playing":@(video.playing),
        @"visible":@(video.panel.isVisible), @"pinned":@(video.pin.state == NSControlStateValueOn),
        @"title":video.title, @"error":video.error, @"id":video.videoID, @"change":@(video.changeRequested)};
    video.changeRequested = NO;
    NSData *data = [NSJSONSerialization dataWithJSONObject:snapshot options:0 error:nil];
    if (!data) return NULL;
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    return strdup(json.UTF8String);
}
void ateliet_video_free_string(char *value) { free(value); }
void ateliet_video_destroy(void *handle) {
    ATWorkVideo *video = (__bridge_transfer ATWorkVideo *)handle;
    [video stop]; video.panel.delegate = nil; [video.panel close];
}
