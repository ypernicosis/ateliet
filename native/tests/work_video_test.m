// Standalone macOS lifecycle test. Run in a graphical login session:
// xcrun clang -fobjc-arc -Wall -Wextra -Wno-unused-parameter \
//   -framework AppKit -framework AVKit -framework AVFoundation \
//   -framework WebKit -framework CoreMedia -framework CoreVideo \
//   native/tests/work_video_test.m -o /tmp/ateliet-video-test
// /tmp/ateliet-video-test native/work_video.html /tmp/ateliet-video-fixture.mov
#import "../work_video.m"
#include <assert.h>

static void spin(double seconds) {
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (until.timeIntervalSinceNow > 0) {
        [NSRunLoop.mainRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
    }
}

static BOOL fixture(NSString *path) {
    [NSFileManager.defaultManager removeItemAtPath:path error:nil];
    NSError *error = nil;
    AVAssetWriter *writer = [[AVAssetWriter alloc] initWithURL:[NSURL fileURLWithPath:path] fileType:AVFileTypeQuickTimeMovie error:&error];
    assert(writer && !error);
    NSDictionary *settings = @{AVVideoCodecKey:AVVideoCodecTypeJPEG, AVVideoWidthKey:@320, AVVideoHeightKey:@180};
    AVAssetWriterInput *input = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:settings];
    AVAssetWriterInputPixelBufferAdaptor *adaptor = [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:input sourcePixelBufferAttributes:@{(id)kCVPixelBufferPixelFormatTypeKey:@(kCVPixelFormatType_32BGRA), (id)kCVPixelBufferWidthKey:@320, (id)kCVPixelBufferHeightKey:@180}];
    [writer addInput:input];
    if (![writer startWriting]) {
        NSLog(@"SKIP native lifecycle: macOS fixture encoder unavailable: %@", writer.error);
        return NO;
    }
    [writer startSessionAtSourceTime:kCMTimeZero];
    for (int frame = 0; frame < 300; frame++) {
        while (!input.readyForMoreMediaData) { assert(writer.status != AVAssetWriterStatusFailed); spin(0.01); }
        CVPixelBufferRef buffer = NULL;
        assert(CVPixelBufferPoolCreatePixelBuffer(NULL, adaptor.pixelBufferPool, &buffer) == kCVReturnSuccess);
        CVPixelBufferLockBaseAddress(buffer, 0);
        uint8_t *pixels = CVPixelBufferGetBaseAddress(buffer);
        size_t stride = CVPixelBufferGetBytesPerRow(buffer);
        for (int y = 0; y < 180; y++) for (int x = 0; x < 320; x++) {
            size_t at = y * stride + x * 4;
            pixels[at] = (x + frame) % 255; pixels[at + 1] = y; pixels[at + 2] = 65; pixels[at + 3] = 255;
        }
        CVPixelBufferUnlockBaseAddress(buffer, 0);
        assert([adaptor appendPixelBuffer:buffer withPresentationTime:CMTimeMake(frame, 30)]);
        CVPixelBufferRelease(buffer);
    }
    [input markAsFinished];
    __block BOOL done = NO;
    [writer finishWritingWithCompletionHandler:^{ done = YES; }];
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:20];
    while (!done && deadline.timeIntervalSinceNow > 0) spin(0.02);
    assert(done && writer.status == AVAssetWriterStatusCompleted);
    return YES;
}

static NSDictionary *snapshot(void *handle) {
    char *json = ateliet_video_snapshot(handle);
    assert(json);
    NSData *data = [[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding];
    ateliet_video_free_string(json);
    return [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
}

// These assertions do not decode media or contact YouTube. They exercise the
// actual AppKit parent/geometry boundary, including egui zoom and return events.
static void layout(void *handle, NSWindow *host) {
    ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
    void *view = (__bridge void *)host.contentView;
    assert(!video.panel && !video.player && !video.web);
    ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1);
    assert(video.surface.superview == host.contentView);
    assert(NSEqualRects(video.surface.frame, NSMakeRect(750, 560, 320, 200)));
    assert(!video.panel); // Default integration must never create a window.
    NSView *surface = video.surface;
    ateliet_video_place(handle, view, 550, 400, 375, 20, 160, 100, 1);
    assert(NSEqualRects(video.surface.frame, NSMakeRect(750, 560, 320, 200)));
    ateliet_video_place(handle, view, 1100, 800, 0, 0, 0, 0, 2);
    assert(video.panel && video.surface == surface && surface.superview == video.panel.contentView);
    [video returnToApp:nil];
    assert(ateliet_video_has_event(handle));
    assert([snapshot(handle)[@"presentation"] isEqual:@"docked"]);
    ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1);
    assert(surface.superview == host.contentView && !video.panel.isVisible);
    ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 0);
    assert(surface.isHidden && !video.playing);
    puts("PASS: native dock, coordinate scaling, detach and return preserve one surface");
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        assert(argc == 3 || argc == 4);
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        [NSApp finishLaunching];
        NSString *html = [NSString stringWithContentsOfFile:[NSString stringWithUTF8String:argv[1]] encoding:NSUTF8StringEncoding error:nil];
        assert(html);
        NSWindow *host = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 1100, 800) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        host.releasedWhenClosed = NO;
        [host orderFront:nil];
        void *view = (__bridge void *)host.contentView;
        void *handle = ateliet_video_create(html.UTF8String);
        layout(handle, host);
        if (argc == 4 && strcmp(argv[3], "--layout") == 0) {
            ateliet_video_destroy(handle); [host close]; return 0;
        }
        NSString *path = [NSString stringWithUTF8String:argv[2]];
        if (!fixture(path)) { ateliet_video_destroy(handle); [host close]; return 77; }
        NSDictionary *source = @{@"source":@{@"kind":@"local", @"path":path}, @"position":@2, @"title":@"Teste de vídeo", @"pinned":@YES};
        NSString *json = [[NSString alloc] initWithData:[NSJSONSerialization dataWithJSONObject:source options:0 error:nil] encoding:NSUTF8StringEncoding];
        ateliet_video_open(handle, json.UTF8String);
        ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1);
        ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:15];
        while (video.player.rate == 0 && deadline.timeIntervalSinceNow > 0) spin(0.05);
        spin(0.3);
        NSDictionary *playing = snapshot(handle);
        assert([playing[@"visible"] boolValue] && [playing[@"playing"] boolValue]);
        assert([playing[@"position"] doubleValue] >= 2);
        AVPlayer *player = video.player;
        AVPlayerView *localView = video.localView;
        ateliet_video_place(handle, view, 1100, 800, 0, 0, 0, 0, 2);
        assert(video.player == player && video.localView == localView);
        assert(video.panel.level == NSFloatingWindowLevel);
        [video returnToApp:nil]; snapshot(handle);
        ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1);
        assert(video.player == player && video.localView == localView);
        // Modal occlusion pauses and resumes only previously playing media.
        ateliet_video_place(handle, view, 1100, 800, 0, 0, 0, 0, 0); spin(0.1);
        assert(![snapshot(handle)[@"playing"] boolValue]);
        ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1); spin(0.1);
        assert([snapshot(handle)[@"playing"] boolValue]);
        ateliet_video_pause(handle);
        double position = [snapshot(handle)[@"position"] doubleValue];
        ateliet_video_place(handle, view, 1100, 800, 0, 0, 0, 0, 0);
        ateliet_video_place(handle, view, 1100, 800, 750, 40, 320, 200, 1); spin(0.2);
        assert(![snapshot(handle)[@"playing"] boolValue]);
        assert(fabs([snapshot(handle)[@"position"] doubleValue] - position) < 0.1);
        ateliet_video_place(handle, view, 1100, 800, 0, 0, 0, 0, 2);
        [video.panel performClose:nil];
        assert([snapshot(handle)[@"presentation"] isEqual:@"collapsed"]);
        assert(video.player == player && !video.playing);
        ateliet_video_open(handle, json.UTF8String);
        [video stop]; // Pending seek completion cannot restart a replaced session.
        spin(0.3);
        assert(!video.player && !video.playing);
        ateliet_video_destroy(handle); [host close];
        puts("PASS: playback, seek, pause, media identity, occlusion, close and stale callbacks");
    }
}
