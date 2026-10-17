/* Test input-method device at AppKit's NSTextInputClient boundary. No DOM events,
 * application commands, system input-source changes or accessibility grants. */
#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>
#include <string.h>

@interface WdioImeContext : NSTextInputContext
@property(nonatomic, weak) NSResponder<NSTextInputClient> *target;
@property(nonatomic, copy) NSString *preedit;
@property(nonatomic, assign) Class originalClass;
@property(nonatomic) BOOL active;
@end
@implementation WdioImeContext
- (BOOL)handleEvent:(NSEvent *)event {
    if (!self.active || event.type != NSEventTypeKeyDown) return NO;
    [self.target setMarkedText:self.preedit selectedRange:NSMakeRange(self.preedit.length, 0)
             replacementRange:NSMakeRange(NSNotFound, 0)];
    return !(event.modifierFlags & (NSEventModifierFlagControl | NSEventModifierFlagCommand));
}
@end

static char CONTEXT_KEY, DEVICE_KEY;
static id native_context(id client, SEL selector) {
    (void)selector;
    return objc_getAssociatedObject(client, &CONTEXT_KEY);
}
static WdioImeContext *device_for(WKWebView *view) {
    return objc_getAssociatedObject(view, &DEVICE_KEY);
}
static void restore(WdioImeContext *device) {
    if (device.active && device.target) {
        object_setClass(device.target, device.originalClass);
        objc_setAssociatedObject(device.target, &CONTEXT_KEY, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    device.active = NO;
}

const char *wdio_ime_set(void *raw, const char *text) {
    WKWebView *view = (__bridge WKWebView *)raw;
    NSResponder *responder = view.window.firstResponder;
    if (![responder conformsToProtocol:@protocol(NSTextInputClient)]) return "The focused WebView is not a native text input client";
    WdioImeContext *device = device_for(view);
    if (!device || device.target != responder) {
        restore(device);
        device = [[WdioImeContext alloc] initWithClient:(id<NSTextInputClient>)responder];
        device.target = (NSResponder<NSTextInputClient> *)responder;
        device.originalClass = object_getClass(responder);
        objc_setAssociatedObject(view, &DEVICE_KEY, device, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    if (!device.active) {
        NSString *name = [@"WdioIme_" stringByAppendingString:NSStringFromClass(device.originalClass)];
        Class subclass = NSClassFromString(name);
        if (!subclass) {
            subclass = objc_allocateClassPair(device.originalClass, name.UTF8String, 0);
            if (!subclass) return "Cannot create the scoped native input-method context";
            class_addMethod(subclass, @selector(inputContext), (IMP)native_context, "@@:");
            objc_registerClassPair(subclass);
        }
        objc_setAssociatedObject(responder, &CONTEXT_KEY, device, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        object_setClass(responder, subclass);
        device.active = YES;
    }
    device.preedit = [NSString stringWithUTF8String:text];
    [device.target setMarkedText:device.preedit selectedRange:NSMakeRange(device.preedit.length, 0)
               replacementRange:NSMakeRange(NSNotFound, 0)];
    return NULL;
}

static int physical_key(const char *key) {
    if (!strcmp(key, "Escape")) return 53;
    if (strlen(key) != 1) return -1;
    const char *letters = "asdfhgzxcvbqwertyuiopljknm";
    const int codes[] = {0,1,2,3,4,5,6,7,8,9,11,12,13,14,15,17,16,32,34,31,35,37,38,40,45,46};
    const char *letter = strchr(letters, key[0]);
    return letter ? codes[letter - letters] : -1;
}
const char *wdio_ime_key(void *raw, const char *key, int control, int meta, const char *preedit_text) {
    WKWebView *view = (__bridge WKWebView *)raw;
    WdioImeContext *device = device_for(view);
    if (!device.active || !device.target) return "No native composition is active";
    if (preedit_text) device.preedit = [NSString stringWithUTF8String:preedit_text];
    int code = physical_key(key);
    if (code < 0) return "Native IME keys must be an ASCII letter or Escape";
    NSString *characters = !strcmp(key, "Escape") ? @"\033" : [NSString stringWithUTF8String:key];
    NSEventModifierFlags modifiers = (control ? NSEventModifierFlagControl : 0) | (meta ? NSEventModifierFlagCommand : 0);
    NSEvent *down = [NSEvent keyEventWithType:NSEventTypeKeyDown location:NSZeroPoint modifierFlags:modifiers
        timestamp:NSProcessInfo.processInfo.systemUptime windowNumber:view.window.windowNumber context:nil
        characters:characters charactersIgnoringModifiers:characters isARepeat:NO keyCode:(unsigned short)code];
    [device.target keyDown:down];
    NSEvent *up = [NSEvent keyEventWithType:NSEventTypeKeyUp location:NSZeroPoint modifierFlags:modifiers
        timestamp:NSProcessInfo.processInfo.systemUptime windowNumber:view.window.windowNumber context:nil
        characters:characters charactersIgnoringModifiers:characters isARepeat:NO keyCode:(unsigned short)code];
    [device.target keyUp:up];
    return NULL;
}
const char *wdio_ime_commit(void *raw, const char *text) {
    WdioImeContext *device = device_for((__bridge WKWebView *)raw);
    if (!device.active || !device.target) return "No native composition is active";
    [device.target insertText:[NSString stringWithUTF8String:text] replacementRange:[device.target markedRange]];
    [device.target unmarkText];
    restore(device);
    return NULL;
}
const char *wdio_ime_cancel(void *raw) {
    WdioImeContext *device = device_for((__bridge WKWebView *)raw);
    if (device.active && device.target) {
        [device.target insertText:@"" replacementRange:[device.target markedRange]];
        [device.target unmarkText];
        restore(device);
    }
    return NULL;
}
