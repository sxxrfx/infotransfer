/* End-to-end benchmark support, compiled only with the `e2e-bench` feature. Right Option
 * events are posted to this process alone (CGEventPostToPid), so no other app, including an
 * installed InfoTransfer, sees them and nothing is activated. They arrive through the
 * WindowServer and the same local monitor and flag reduction that native/macos.m uses. */
#import <AppKit/AppKit.h>
#import <IOKit/hidsystem/IOLLEvent.h>
#include <time.h>
#include <unistd.h>

typedef void (*ItBenchKey)(uint16_t code, uint8_t flags, int key_down, int pid, uint64_t event_ns,
                           uint64_t received_ns);
static ItBenchKey benchKey;
static id benchMonitor;

uint64_t it_bench_now_ns(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }

void it_bench_app_init(void) {
    [NSApplication sharedApplication];
    NSApp.activationPolicy = NSApplicationActivationPolicyAccessory;
    [NSApp finishLaunching];
}

int it_bench_monitor(ItBenchKey callback) {
    benchKey = callback;
    benchMonitor = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskFlagsChanged | NSEventMaskKeyDown
                                                         handler:^NSEvent *(NSEvent *event) {
        uint64_t received = it_bench_now_ns();
        // Same reduction as observeKey in macos.m: Fn=1, right Option=2, others=4.
        NSEventModifierFlags flags = event.modifierFlags;
        uint8_t modifiers = 0;
        if (flags & NSEventModifierFlagFunction) modifiers |= 1;
        if (flags & NX_DEVICERALTKEYMASK) modifiers |= 2;
        if ((flags & (NSEventModifierFlagShift | NSEventModifierFlagControl | NSEventModifierFlagCommand | NX_DEVICELALTKEYMASK)) ||
            ((flags & NSEventModifierFlagOption) && !(flags & NX_DEVICERALTKEYMASK))) modifiers |= 4;
        // NSEvent timestamps are seconds of CLOCK_UPTIME_RAW at event creation.
        uint64_t stamp = (uint64_t)(event.timestamp * 1e9);
        if (benchKey) benchKey(event.keyCode, modifiers, event.type == NSEventTypeKeyDown,
                               NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier, stamp, received);
        return event;
    }];
    return benchMonitor != nil;
}

/* Posts right Option (key code 61) as a flags-changed event to this process only. */
uint64_t it_bench_post_right_option(int down) {
    static CGEventSourceRef source;
    if (!source) source = CGEventSourceCreate(kCGEventSourceStatePrivate);
    CGEventRef event = CGEventCreateKeyboardEvent(source, 61, down != 0);
    CGEventSetType(event, kCGEventFlagsChanged);
    CGEventSetFlags(event, down ? (kCGEventFlagMaskAlternate | NX_DEVICERALTKEYMASK) : 0);
    uint64_t now = it_bench_now_ns();
    CGEventPostToPid(getpid(), event);
    CFRelease(event);
    return now;
}

/* Local monitors fire only while AppKit's event queue is drained, as GPUI's run loop does. */
void it_bench_pump(double seconds) {
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:seconds];
    for (;;) {
        NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:until inMode:NSDefaultRunLoopMode dequeue:YES];
        if (!event) break;
        [NSApp sendEvent:event];
    }
}
