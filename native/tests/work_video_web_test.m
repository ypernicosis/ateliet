// Runs the production wrapper in JavaScriptCore with an asynchronous player
// double. No web process, network, decoder or browser is required.
// xcrun clang -fobjc-arc -framework Foundation -framework JavaScriptCore \
//   native/tests/work_video_web_test.m -o /tmp/ateliet-video-web-test
// /tmp/ateliet-video-web-test native/work_video.html
#import <Foundation/Foundation.h>
#import <JavaScriptCore/JavaScriptCore.h>
#include <assert.h>

static void run(JSContext *ctx, NSString *script) {
    [ctx evaluateScript:script];
    if (ctx.exception) { NSLog(@"%@", ctx.exception); exit(1); }
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        assert(argc == 2);
        NSString *html = [NSString stringWithContentsOfFile:[NSString stringWithUTF8String:argv[1]] encoding:NSUTF8StringEncoding error:nil];
        assert(html);
        NSRange start = [html rangeOfString:@"<script>"];
        NSRange end = [html rangeOfString:@"</script>"];
        NSString *script = [html substringWithRange:NSMakeRange(NSMaxRange(start), end.location - NSMaxRange(start))];
        script = [script stringByReplacingOccurrencesOfString:@"__ATELIET_CONFIG__" withString:@"{id:'dQw4w9WgXcQ',position:42,origin:'https://md.ateliet.app'}"];
        JSContext *ctx = [JSContext new];
        run(ctx, @"var calls=[], messages=[], timers=[], options, media;"
            "var window=globalThis;"
            "window.webkit={messageHandlers:{video:{postMessage:m=>messages.push(m)}}};"
            "var document={createElement:()=>({}),head:{appendChild:()=>{}}};"
            "var setInterval=f=>timers.push(f),setTimeout=()=>{};"
            "var YT={Player:function(id,o){options=o;media={state:5,getPlayerState(){return this.state},"
            "playVideo(){calls.push('play')},pauseVideo(){calls.push('pause')},"
            "getCurrentTime(){return 42},getVideoData(){return {title:'Reference',video_id:'dQw4w9WgXcQ'}}};return media;}};"
            "function emit(n){media.state=n;options.events.onStateChange({data:n});}"
            "function check(test,label){if(!test)throw new Error(label);}"
        );
        run(ctx, script);
        // Native commands can precede both the iframe API and its ready event.
        run(ctx, @"atelietSetState(1,true,true);atelietSetState(2,false,false);"
            "onYouTubeIframeAPIReady();options.events.onReady();"
            "check(calls.length===0,'collapsed before ready must never autoplay');"
            "check(options.playerVars.autoplay===0,'no independent autoplay');"
            "atelietSetState(3,true,true);check(calls.at(-1)==='play','explicit restore');"
            "emit(3);emit(1);"
            "var count=calls.length;atelietSetState(2,false,false);"
            "check(calls.length===count,'stale command must be ignored');"
        );
        // Buffering retains intent. Temporary suspension is not a user pause.
        run(ctx, @"emit(3);check(messages.at(-1).intent===true,'buffering preserves intent');"
            "atelietSetState(4,true,false);check(calls.at(-1)==='pause','hide pauses buffering');"
            "emit(2);atelietSetState(5,true,true);check(calls.at(-1)==='play','buffering resumes');"
            "emit(1);emit(2);check(messages.at(-1).intent===false,'manual pause');"
            "count=calls.length;atelietSetState(6,false,false);atelietSetState(7,false,true);"
            "check(calls.length===count,'manual pause survives hide/show');"
        );
        // A late acknowledgement from hide must not undo a fast restoration.
        run(ctx, @"atelietSetState(8,true,true);emit(1);"
            "atelietSetState(9,true,false);atelietSetState(10,true,true);emit(2);"
            "check(messages.at(-1).intent===true,'late pause acknowledgement retains intent');"
            "check(calls.at(-1)==='play','late pause converges to current intent');emit(1);"
            "atelietSetState(11,false,true);emit(2);"
            "check(messages.at(-1).intent===false,'explicit pause wins');"
            "emit(1);check(messages.at(-1).intent===true,'native YouTube controls can play');"
            "emit(0);check(messages.at(-1).intent===false,'end is not buffering');"
            "timers[0]();check(messages.at(-1).position===0,'ended position resets');"
            "options.events.onError({data:150});check(messages.at(-1).error.includes('autor'),'actionable embed error');"
        );
        run(ctx, @"atelietSetState(12,true,true);emit(1);"
            "atelietSetState(13,true,false);count=calls.length;"
            "atelietSetState(14,true,false);atelietSetState(15,true,false);"
            "check(calls.length===count,'duplicate hidden commands must not queue pause acknowledgements');"
            "emit(2);atelietSetState(16,true,true);emit(1);emit(2);"
            "check(messages.at(-1).intent===false,'manual pause must work after repeated suspension commands');"
        );
        puts("PASS: production YouTube wrapper: readiness, buffering, manual pause, fast hide/show, stale commands, end and errors");
    }
}
