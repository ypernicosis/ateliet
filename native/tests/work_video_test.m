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

int main(int argc, const char **argv) {
    @autoreleasepool {
        assert(argc == 3);
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        [NSApp finishLaunching];
        NSString *html = [NSString stringWithContentsOfFile:[NSString stringWithUTF8String:argv[1]] encoding:NSUTF8StringEncoding error:nil];
        assert(html);
        NSString *path = [NSString stringWithUTF8String:argv[2]];
        if (!fixture(path)) return 77; // Infrastructure unavailable, not a passing playback test.
        void *handle = ateliet_video_create(html.UTF8String);
        NSDictionary *source = @{@"source":@{@"kind":@"local", @"path":path}, @"position":@2, @"title":@"Teste de vídeo", @"pinned":@YES};
        NSString *json = [[NSString alloc] initWithData:[NSJSONSerialization dataWithJSONObject:source options:0 error:nil] encoding:NSUTF8StringEncoding];
        ateliet_video_open(handle, json.UTF8String);
        ATWorkVideo *video = (__bridge ATWorkVideo *)handle;
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:15];
        while (video.player.rate == 0 && deadline.timeIntervalSinceNow > 0) spin(0.05);
        spin(0.3);
        NSDictionary *playing = snapshot(handle);
        assert([playing[@"visible"] boolValue] && [playing[@"playing"] boolValue]);
        assert([playing[@"position"] doubleValue] >= 2);
        assert(video.panel.level == NSFloatingWindowLevel);
        ateliet_video_toggle(handle); spin(0.1);
        NSDictionary *paused = snapshot(handle);
        assert(![paused[@"playing"] boolValue]);
        double position = [paused[@"position"] doubleValue];
        spin(0.2);
        assert(fabs([snapshot(handle)[@"position"] doubleValue] - position) < 0.1);
        [video.panel performClose:nil]; spin(0.1);
        assert(![snapshot(handle)[@"visible"] boolValue]);
        assert(!video.player && !video.localView && !video.web);
        ateliet_video_open(handle, json.UTF8String); spin(0.1);
        assert(video.player && video.panel.isVisible);
        [video stop]; // Pending seek completion must not restart a replaced session.
        spin(0.3);
        assert(!video.player && !video.playing);
        ateliet_video_destroy(handle);
        puts("PASS: native playback, seek, pause, pin, close, release, reopen and stale callback lifecycle");
    }
}
