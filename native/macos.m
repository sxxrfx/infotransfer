#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#import <IOKit/hidsystem/IOLLEvent.h>
#import <ServiceManagement/ServiceManagement.h>

typedef void (*ItEvent)(int, int);
typedef void (*ItKeyEvent)(uint16_t, uint8_t, int, int);
typedef int (*ItDictionaryCheck)(const char *, int, char *, size_t);
static ItEvent eventCallback;
static ItKeyEvent keyCallback;
static NSStatusItem *statusItem;
static NSMenuItem *playbackItem;
static NSMenuItem *stopItem;
static NSWindow *playerWindow;
static id globalKeyMonitor;
static id localKeyMonitor;
static EventHotKeyRef escapeShortcut;
static NSTextField *searchField;
static NSView *searchHost;
static NSView *searchContainer;
static NSView *searchViewport;
static BOOL lastAccessibility;
static int lastFnPolicy;
static NSPanel *dictionaryPanel;
static NSTextField *dictionaryWord;
static NSTextField *dictionaryReplacement;
static NSTextField *dictionaryError;
static NSButton *dictionaryCase;
static NSData *dictionaryResult;
static int dictionaryIndex;
static ItDictionaryCheck dictionaryCheck;
void it_native_dictionary_feedback(const char *error);
int it_native_activation_monitor(void);
int it_native_fn_unassigned(void);

static pid_t frontPid(void) { return NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier; }
int it_native_activate_existing(void) {
    pid_t own = NSProcessInfo.processInfo.processIdentifier;
    for (NSRunningApplication *application in NSWorkspace.sharedWorkspace.runningApplications) {
        NSString *identifier = application.bundleIdentifier;
        if (application.processIdentifier != own && !application.terminated &&
            ([identifier isEqualToString:@"dev.infotransfer.reader"] ||
             [identifier hasPrefix:@"dev.infotransfer.reader.preview"])) {
            [application activateWithOptions:0];
            return 1;
        }
    }
    return 0;
}
static void emit(int action, pid_t pid) {
    if (eventCallback) eventCallback(action, pid);
}

@interface ItMenuTarget : NSObject <NSTextFieldDelegate>
- (void)invoke:(NSMenuItem *)sender;
- (void)pause:(NSNotification *)notification;
- (void)displayOptions:(NSNotification *)notification;
- (void)accessChanged:(NSNotification *)notification;
- (void)sourceChanged:(NSNotification *)notification;
- (void)saveWord:(id)sender;
- (void)cancelWord:(id)sender;
@end
@implementation ItMenuTarget
- (void)invoke:(NSMenuItem *)sender { emit((int)sender.tag, frontPid()); }
- (void)pause:(NSNotification *)notification { emit(7, 0); }
- (void)displayOptions:(NSNotification *)notification { emit(13, 0); }
- (void)accessChanged:(NSNotification *)notification { emit(19, 0); }
- (void)sourceChanged:(NSNotification *)notification { emit(22, 0); }
- (void)saveWord:(id)sender {
    (void)sender;
    NSDictionary *entry = @{@"word":dictionaryWord.stringValue, @"replacement":dictionaryReplacement.stringValue,
        @"match_case":dictionaryCase.state == NSControlStateValueOn ? @YES : @NO};
    dictionaryResult = [NSJSONSerialization dataWithJSONObject:entry options:0 error:nil];
    NSString *json = [[NSString alloc] initWithData:dictionaryResult encoding:NSUTF8StringEncoding];
    char error[512] = {0};
    if (!dictionaryCheck || !dictionaryCheck(json.UTF8String, dictionaryIndex, error, sizeof(error))) {
        it_native_dictionary_feedback(error[0] ? error : "Could not save this replacement");
        return;
    }
    // GPUI foreground tasks do not drain during AppKit's modal sheet loop.
    // Validate through a pure Rust callback, then close before enqueueing the edit.
    it_native_dictionary_feedback(NULL);
    emit(23, dictionaryIndex);
}
- (void)cancelWord:(id)sender {
    (void)sender;
    [dictionaryPanel.sheetParent endSheet:dictionaryPanel];
    [dictionaryPanel orderOut:nil]; dictionaryPanel = nil; dictionaryResult = nil;
}
- (void)controlTextDidChange:(NSNotification *)notification { emit(14, 0); }
- (void)controlTextDidBeginEditing:(NSNotification *)notification { emit(15, 0); }
- (void)controlTextDidEndEditing:(NSNotification *)notification { emit(16, 0); }
- (BOOL)control:(NSControl *)control textView:(NSTextView *)textView doCommandBySelector:(SEL)selector {
    if (selector == @selector(insertTab:) || selector == @selector(insertBacktab:)) {
        [searchField.window makeFirstResponder:searchHost];
        emit(selector == @selector(insertTab:) ? 17 : 18, 0);
        return YES;
    }
    return NO;
}
@end
static ItMenuTarget *menuTarget;

static OSStatus hotkey(EventHandlerCallRef next, EventRef event, void *context) {
    (void)next; (void)context;
    EventHotKeyID identifier;
    if (GetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID, NULL,
                          sizeof(identifier), NULL, &identifier) == noErr) {
        UInt32 kind = GetEventKind(event);
        if (identifier.id == 4 && kind == kEventHotKeyPressed) emit(12, frontPid());
    }
    return noErr;
}

static void observeKey(NSEvent *event) {
    if (!keyCallback) return;
    NSEventModifierFlags flags = event.modifierFlags;
    uint8_t modifiers = 0;
    if (flags & NSEventModifierFlagFunction) modifiers |= 1;
    if (flags & NX_DEVICERALTKEYMASK) modifiers |= 2;
    if ((flags & (NSEventModifierFlagShift | NSEventModifierFlagControl | NSEventModifierFlagCommand | NX_DEVICELALTKEYMASK)) ||
        ((flags & NSEventModifierFlagOption) && !(flags & NX_DEVICERALTKEYMASK))) modifiers |= 4;
    keyCallback(event.keyCode, modifiers, event.type == NSEventTypeKeyDown, frontPid());
}

void it_native_install(ItEvent callback, ItKeyEvent key_callback) {
    eventCallback = callback;
    keyCallback = key_callback;
    NSApp.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
    menuTarget = [ItMenuTarget new];
    NSMenu *edit = [[NSMenu alloc] initWithTitle:@"Edit"];
    NSArray *editTitles = @[@"Undo", @"Redo", @"Cut", @"Copy", @"Paste", @"Select All"];
    NSArray *editSelectors = @[@"undo:", @"redo:", @"cut:", @"copy:", @"paste:", @"selectAll:"];
    NSArray *editKeys = @[@"z", @"z", @"x", @"c", @"v", @"a"];
    for (NSUInteger index = 0; index < editTitles.count; index++) {
        if (index == 2 || index == 5) [edit addItem:NSMenuItem.separatorItem];
        NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:editTitles[index]
            action:NSSelectorFromString(editSelectors[index]) keyEquivalent:editKeys[index]];
        item.keyEquivalentModifierMask = NSEventModifierFlagCommand | (index == 1 ? NSEventModifierFlagShift : 0);
        [edit addItem:item];
    }
    NSMenuItem *editItem = [[NSMenuItem alloc] initWithTitle:@"Edit" action:nil keyEquivalent:@""];
    editItem.submenu = edit;
    [NSApp.mainMenu addItem:editItem];
    statusItem = [NSStatusBar.systemStatusBar statusItemWithLength:NSSquareStatusItemLength];
    statusItem.button.image = [NSImage imageWithSystemSymbolName:@"speaker.wave.2" accessibilityDescription:@"InfoTransfer"];
    NSMenu *menu = [NSMenu new];
    NSArray *titles = @[@"Read selection", @"Read clipboard", @"Pause / Resume", @"Stop", @"Settings…", @"Quit InfoTransfer"];
    for (int i = 0; i < 6; i++) {
        if (i == 4 || i == 5) [menu addItem:NSMenuItem.separatorItem];
        NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:titles[i] action:@selector(invoke:) keyEquivalent:@""];
        item.target = menuTarget; item.tag = i + 1; [menu addItem:item];
        if (i == 2) playbackItem = item;
        if (i == 3) stopItem = item;
    }
    statusItem.menu = menu;
    menu.autoenablesItems = NO;
    playbackItem.enabled = NO;
    stopItem.enabled = NO;
    EventTypeSpec types[] = {{kEventClassKeyboard, kEventHotKeyPressed}, {kEventClassKeyboard, kEventHotKeyReleased}};
    InstallApplicationEventHandler(hotkey, 2, types, NULL, NULL);
    [NSWorkspace.sharedWorkspace.notificationCenter addObserver:menuTarget selector:@selector(pause:)
        name:NSWorkspaceWillSleepNotification object:nil];
    [NSWorkspace.sharedWorkspace.notificationCenter addObserver:menuTarget selector:@selector(sourceChanged:)
        name:NSWorkspaceDidActivateApplicationNotification object:nil];
    [NSWorkspace.sharedWorkspace.notificationCenter addObserver:menuTarget selector:@selector(displayOptions:)
        name:NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification object:nil];
    [NSNotificationCenter.defaultCenter addObserver:menuTarget selector:@selector(accessChanged:)
        name:NSApplicationDidBecomeActiveNotification object:nil];
    [NSDistributedNotificationCenter.defaultCenter addObserver:menuTarget selector:@selector(pause:)
        name:@"com.apple.screenIsLocked" object:nil];
    lastAccessibility = AXIsProcessTrusted();
    lastFnPolicy = it_native_fn_unassigned();
    NSTimer *permissionTimer = [NSTimer timerWithTimeInterval:1.0 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        BOOL allowed = AXIsProcessTrusted();
        int fnPolicy = it_native_fn_unassigned();
        if (allowed != lastAccessibility || fnPolicy != lastFnPolicy) {
            lastAccessibility = allowed;
            lastFnPolicy = fnPolicy;
            it_native_activation_monitor();
            emit(19, 0);
        }
    }];
    [NSRunLoop.mainRunLoop addTimer:permissionTimer forMode:NSRunLoopCommonModes];
    [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDown handler:^NSEvent *(NSEvent *event) {
        if (event.type == NSEventTypeLeftMouseDown && event.window == searchField.window && searchField.currentEditor) {
            NSPoint point = [searchViewport convertPoint:event.locationInWindow fromView:nil];
            if (!NSPointInRect(point, searchViewport.bounds)) [searchField.window makeFirstResponder:searchHost];
        }
        return event;
    }];
}

int it_native_fn_unassigned(void) {
    // Read-only, best-effort detection of the system Fn action. This preference
    // is undocumented: missing/unknown values conservatively use right Option.
    CFPropertyListRef value = CFPreferencesCopyAppValue(CFSTR("AppleFnUsageType"), CFSTR("com.apple.HIToolbox"));
    int status = -1;
    if (value && CFGetTypeID(value) == CFNumberGetTypeID()) {
        int action;
        if (CFNumberGetValue((CFNumberRef)value, kCFNumberIntType, &action)) status = action == 0 ? 1 : 0;
    }
    if (value) CFRelease(value);
    return status;
}
int it_native_activation_monitor(void) {
    if (!AXIsProcessTrusted()) {
        if (globalKeyMonitor) { [NSEvent removeMonitor:globalKeyMonitor]; globalKeyMonitor = nil; }
        if (localKeyMonitor) { [NSEvent removeMonitor:localKeyMonitor]; localKeyMonitor = nil; }
        emit(22, 0);
        return 0;
    }
    NSEventMask mask = NSEventMaskFlagsChanged | NSEventMaskKeyDown;
    if (!globalKeyMonitor) globalKeyMonitor = [NSEvent addGlobalMonitorForEventsMatchingMask:mask handler:^(NSEvent *event) { observeKey(event); }];
    if (!localKeyMonitor) localKeyMonitor = [NSEvent addLocalMonitorForEventsMatchingMask:mask handler:^NSEvent *(NSEvent *event) {
        observeKey(event);
        return event;
    }];
    return globalKeyMonitor != nil && localKeyMonitor != nil;
}
int it_native_escape(int active) {
    if (active && escapeShortcut) return 1;
    if (escapeShortcut) { UnregisterEventHotKey(escapeShortcut); escapeShortcut = NULL; }
    if (!active) return 1;
    EventHotKeyID identifier = {'IfTr', 4};
    return RegisterEventHotKey(53, 0, identifier, GetApplicationEventTarget(), 0, &escapeShortcut) == noErr;
}
void it_native_playback_state(int paused, int active, int can_stop) {
    playbackItem.title = paused ? @"Resume" : @"Pause";
    playbackItem.enabled = active;
    stopItem.enabled = can_stop;
}
int it_native_ax_trusted(void) { return AXIsProcessTrusted(); }
int it_native_open_at_login(void) { return SMAppService.mainAppService.status == SMAppServiceStatusEnabled; }
int it_native_set_open_at_login(int enabled) {
    NSError *error = nil;
    return enabled ? [SMAppService.mainAppService registerAndReturnError:&error]
                   : [SMAppService.mainAppService unregisterAndReturnError:&error];
}
static NSTextField *dictionaryLabel(NSString *text, NSRect frame) {
    NSTextField *label = [NSTextField labelWithString:text];
    label.frame = frame; label.font = [NSFont systemFontOfSize:13.125 weight:NSFontWeightMedium];
    return label;
}
void it_native_dictionary_edit(void *view, int index, const char *word, const char *replacement, int matchCase, ItDictionaryCheck check) {
    if (dictionaryPanel) return;
    dictionaryIndex = index; dictionaryResult = nil;
    dictionaryCheck = check;
    NSWindow *parent = ((__bridge NSView *)view).window;
    dictionaryPanel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, 420, 270)
        styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    dictionaryPanel.title = @"Word replacement";
    dictionaryPanel.appearance = [NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];
    NSView *content = dictionaryPanel.contentView;
    [content addSubview:dictionaryLabel(@"Word or abbreviation", NSMakeRect(20, 230, 380, 20))];
    dictionaryWord = [[NSTextField alloc] initWithFrame:NSMakeRect(20, 200, 380, 26)];
    dictionaryWord.stringValue = [NSString stringWithUTF8String:word]; dictionaryWord.accessibilityLabel = @"Word or abbreviation";
    dictionaryWord.font = [NSFont systemFontOfSize:13.125];
    dictionaryWord.cell.usesSingleLineMode = YES; dictionaryWord.cell.scrollable = YES;
    [content addSubview:dictionaryWord];
    [content addSubview:dictionaryLabel(@"Read as", NSMakeRect(20, 167, 380, 20))];
    dictionaryReplacement = [[NSTextField alloc] initWithFrame:NSMakeRect(20, 137, 380, 26)];
    dictionaryReplacement.stringValue = [NSString stringWithUTF8String:replacement]; dictionaryReplacement.accessibilityLabel = @"Read as";
    dictionaryReplacement.font = [NSFont systemFontOfSize:13.125];
    dictionaryReplacement.cell.usesSingleLineMode = YES; dictionaryReplacement.cell.scrollable = YES;
    [content addSubview:dictionaryReplacement];
    dictionaryCase = [NSButton checkboxWithTitle:@"Match case" target:nil action:nil];
    dictionaryCase.frame = NSMakeRect(20, 100, 250, 24); dictionaryCase.state = matchCase ? NSControlStateValueOn : NSControlStateValueOff;
    [content addSubview:dictionaryCase];
    dictionaryError = [NSTextField wrappingLabelWithString:@""]; dictionaryError.frame = NSMakeRect(20, 56, 380, 36);
    dictionaryError.font = [NSFont systemFontOfSize:12]; dictionaryError.textColor = NSColor.systemRedColor;
    [content addSubview:dictionaryError];
    NSButton *cancel = [NSButton buttonWithTitle:@"Cancel" target:menuTarget action:@selector(cancelWord:)];
    cancel.frame = NSMakeRect(220, 16, 86, 28); cancel.keyEquivalent = @"\e"; [content addSubview:cancel];
    NSButton *save = [NSButton buttonWithTitle:@"Save" target:menuTarget action:@selector(saveWord:)];
    save.frame = NSMakeRect(314, 16, 86, 28); save.keyEquivalent = @"\r"; [content addSubview:save];
    dictionaryPanel.defaultButtonCell = save.cell;
    [parent beginSheet:dictionaryPanel completionHandler:nil];
    [dictionaryPanel makeFirstResponder:dictionaryWord];
}
char *it_native_dictionary_result(void) {
    if (!dictionaryResult) return NULL;
    NSString *text = [[NSString alloc] initWithData:dictionaryResult encoding:NSUTF8StringEncoding];
    dictionaryResult = nil; return strdup(text.UTF8String);
}
void it_native_dictionary_feedback(const char *error) {
    if (!dictionaryPanel) return;
    if (error) {
        dictionaryError.stringValue = [NSString stringWithUTF8String:error];
        NSAccessibilityPostNotificationWithUserInfo(NSApp, NSAccessibilityAnnouncementRequestedNotification,
            @{NSAccessibilityAnnouncementKey:dictionaryError.stringValue, NSAccessibilityPriorityKey:@(NSAccessibilityPriorityHigh)});
        return;
    }
    [dictionaryPanel.sheetParent endSheet:dictionaryPanel];
    [dictionaryPanel orderOut:nil]; dictionaryPanel = nil;
}
void it_native_request_ax(void) {
    AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt:@YES});
    [NSWorkspace.sharedWorkspace openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]];
}
void it_native_access_settings(void) {
    [NSWorkspace.sharedWorkspace openURL:[NSURL URLWithString:@"x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]];
}
static int selectionString(AXUIElementRef element, char **output) {
    CFTypeRef role = NULL;
    if (AXUIElementCopyAttributeValue(element, kAXSubroleAttribute, &role) == kAXErrorSuccess) {
        BOOL secure = CFGetTypeID(role) == CFStringGetTypeID() && CFEqual(role, CFSTR("AXSecureTextField"));
        CFRelease(role); if (secure) return 4;
    }
    CFTypeRef value = NULL;
    AXError result = AXUIElementCopyAttributeValue(element, kAXSelectedTextAttribute, &value);
    if (result != kAXErrorSuccess || !value) {
        // Firefox-based browsers such as Zen only expose page selections as text markers.
        CFTypeRef markers = NULL;
        if (AXUIElementCopyAttributeValue(element, CFSTR("AXSelectedTextMarkerRange"), &markers) != kAXErrorSuccess || !markers) return 2;
        result = AXUIElementCopyParameterizedAttributeValue(element, CFSTR("AXStringForTextMarkerRange"), markers, &value);
        CFRelease(markers);
        if (result != kAXErrorSuccess || !value) return 2;
    }
    if (CFGetTypeID(value) != CFStringGetTypeID()) { CFRelease(value); return 2; }
    CFIndex length = CFStringGetLength(value);
    if (length > 200000) { CFRelease(value); return 5; }
    CFIndex bytes = CFStringGetMaximumSizeForEncoding(length, kCFStringEncodingUTF8) + 1;
    char *buffer = malloc((size_t)bytes);
    if (!buffer) { CFRelease(value); return 6; }
    BOOL valid = CFStringGetCString(value, buffer, bytes, kCFStringEncodingUTF8);
    CFRelease(value);
    if (!valid) { free(buffer); return 6; }
    *output = buffer; return 0;
}
// Called on the capture worker. Native callbacks only enqueue small events.
int it_native_selection(int pid, char **output, int64_t *start, int64_t *length) {
    *start = -1; *length = -1;
    if (!AXIsProcessTrusted()) return 1;
    AXUIElementRef app = AXUIElementCreateApplication(pid);
    AXUIElementSetMessagingTimeout(app, 0.08f);
    CFTypeRef focused = NULL;
    AXError error = AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute, &focused);
    int status = 2;
    if (error == kAXErrorSuccess && focused && CFGetTypeID(focused) == AXUIElementGetTypeID()) {
        AXUIElementSetMessagingTimeout((AXUIElementRef)focused, 0.08f);
        status = selectionString((AXUIElementRef)focused, output);
        if (status == 0) {
            CFTypeRef selectedRange = NULL;
            if (AXUIElementCopyAttributeValue((AXUIElementRef)focused, kAXSelectedTextRangeAttribute, &selectedRange) == kAXErrorSuccess && selectedRange) {
                CFRange range;
                if (CFGetTypeID(selectedRange) == AXValueGetTypeID() && AXValueGetType((AXValueRef)selectedRange) == kAXValueCFRangeType && AXValueGetValue((AXValueRef)selectedRange, kAXValueCFRangeType, &range)) {
                    *start = range.location; *length = range.length;
                }
                CFRelease(selectedRange);
            }
        }
    }
    if (focused) CFRelease(focused);
    CFRelease(app);
    return status;
}
void it_native_player(void *view) {
    playerWindow = ((__bridge NSView *)view).window;
    playerWindow.title = @"InfoTransfer Player";
    playerWindow.level = NSFloatingWindowLevel;
    playerWindow.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    playerWindow.animationBehavior = NSWindowAnimationBehaviorNone;
    playerWindow.hidesOnDeactivate = NO;
    playerWindow.hasShadow = NO;
    playerWindow.ignoresMouseEvents = YES;
}
void it_native_player_visible(int visible) {
    if (visible) [playerWindow orderFrontRegardless]; else [playerWindow orderOut:nil];
}
void it_native_window_visible(void *view, int visible) {
    NSWindow *window = ((__bridge NSView *)view).window;
    if (visible) [window makeKeyAndOrderFront:nil]; else [window orderOut:nil];
}
unsigned it_native_source_display(int pid) {
    CFArrayRef list = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
    CGRect bounds = CGRectNull;
    if (list) {
        for (NSDictionary *item in (__bridge NSArray *)list) {
            if ([item[(__bridge NSString *)kCGWindowOwnerPID] intValue] == pid &&
                [item[(__bridge NSString *)kCGWindowLayer] intValue] == 0) {
                CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)item[(__bridge NSString *)kCGWindowBounds], &bounds);
                break;
            }
        }
        CFRelease(list);
    }
    CGDirectDisplayID displays[16]; uint32_t count = 0;
    CGGetActiveDisplayList(16, displays, &count);
    CGFloat best = 0; unsigned selected = 0;
    for (unsigned i = 0; i < count && !CGRectIsNull(bounds); i++) {
        CGRect intersection = CGRectIntersection(bounds, CGDisplayBounds(displays[i]));
        CGFloat area = intersection.size.width * intersection.size.height;
        if (!CGRectIsNull(intersection) && area > best) { best = area; selected = displays[i]; }
    }
    return selected;
}
void it_native_player_position(unsigned display) {
    NSPoint point = NSEvent.mouseLocation;
    NSScreen *screen = NSScreen.mainScreen;
    for (NSScreen *candidate in NSScreen.screens) {
        BOOL matches = display ? [candidate.deviceDescription[@"NSScreenNumber"] unsignedIntValue] == display : NSPointInRect(point, candidate.frame);
        if (matches) { screen = candidate; break; }
    }
    NSRect frame = screen.visibleFrame;
    [playerWindow setFrameOrigin:NSMakePoint(NSMidX(frame) - playerWindow.frame.size.width / 2, frame.origin.y + 18)];
}
int it_native_reduced_motion(void) { return NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion; }

void it_native_search_layout(void *view, double x, double y, double width, double height,
                             double clipX, double clipY, double clipWidth, double clipHeight) {
    NSView *host = (__bridge NSView *)view;
    if (!searchField) {
        searchField = [[NSTextField alloc] initWithFrame:NSZeroRect];
        searchField.bezeled = NO; searchField.bordered = NO; searchField.drawsBackground = NO;
        searchField.focusRingType = NSFocusRingTypeNone;
        searchField.font = [NSFont systemFontOfSize:13.125 weight:NSFontWeightRegular];
        searchField.textColor = [NSColor colorWithSRGBRed:251./255 green:251./255 blue:251./255 alpha:1];
        searchField.placeholderAttributedString = [[NSAttributedString alloc] initWithString:@"Search voices…" attributes:@{
            NSForegroundColorAttributeName:[NSColor colorWithSRGBRed:251./255 green:251./255 blue:251./255 alpha:0.4],
            NSFontAttributeName:searchField.font
        }];
        searchField.delegate = menuTarget;
        searchField.cell.usesSingleLineMode = YES; searchField.cell.scrollable = YES; searchField.cell.wraps = NO;
        searchField.accessibilityLabel = @"Search voices";
        searchField.accessibilityIdentifier = @"voice-search";
        searchViewport = [[NSView alloc] initWithFrame:NSZeroRect];
        searchViewport.clipsToBounds = YES;
        [searchViewport addSubview:searchField];
    }
    if (searchHost != host) {
        [searchViewport removeFromSuperview];
        NSWindow *window = host.window;
        NSView *root = window.contentView;
        // Keep native controls beside GPUI's custom accessibility root, rather
        // than beneath the root's intentionally virtual children.
        NSResponder *responder = window.firstResponder;
        NSView *container = [[NSView alloc] initWithFrame:root.frame];
        container.accessibilityElement = YES;
        container.accessibilityRole = NSAccessibilityGroupRole;
        window.contentView = container;
        root.frame = container.bounds;
        root.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        [container addSubview:root];
        if (responder) [window makeFirstResponder:responder];
        searchHost = host;
        searchContainer = container;
        [container addSubview:searchViewport];
    }
    CGFloat top = host.isFlipped ? y : host.bounds.size.height - y - height;
    NSRect rect = [host convertRect:NSMakeRect(x, top, width, height) toView:searchContainer];
    CGFloat clipTop = host.isFlipped ? clipY : host.bounds.size.height - clipY - clipHeight;
    NSRect mask = [host convertRect:NSMakeRect(clipX, clipTop, clipWidth, clipHeight) toView:searchContainer];
    NSRect clipped = NSIntersectionRect(rect, mask);
    BOOL visible = clipped.size.width > 0 && clipped.size.height > 0;
    if (!visible && searchField.currentEditor) [searchField.window makeFirstResponder:searchHost];
    searchViewport.hidden = !visible;
    searchViewport.frame = clipped;
    rect.origin.x -= clipped.origin.x;
    rect.origin.y -= clipped.origin.y;
    if (!NSEqualRects(searchField.frame, rect)) searchField.frame = rect;
    searchField.hidden = !visible;
}
void it_native_search_visible(int visible) {
    if (!visible && searchField.currentEditor) [searchField.window makeFirstResponder:searchHost];
    searchField.hidden = !visible;
    searchViewport.hidden = !visible;
}
void it_native_search_focus(void) { [searchField.window makeFirstResponder:searchField]; }
char *it_native_search_text(void) { return strdup(searchField.stringValue.UTF8String ?: ""); }
void it_native_search_clear(void) {
    searchField.stringValue = @"";
    if (searchField.currentEditor) searchField.currentEditor.string = @"";
}
