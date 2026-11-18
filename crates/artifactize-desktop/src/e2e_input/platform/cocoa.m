/* Engine-native NSTextInputClient composition, called only on the WebView UI thread. */
#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>

@interface ArtiIme : NSObject
@property(nonatomic, weak) NSResponder<NSTextInputClient> *target;
@property(nonatomic) BOOL active;
@end
@implementation ArtiIme
@end

static char DEVICE_KEY;
static ArtiIme *device_for(WKWebView *view) {
    ArtiIme *device = objc_getAssociatedObject(view, &DEVICE_KEY);
    if (!device) {
        device = [[ArtiIme alloc] init];
        objc_setAssociatedObject(view, &DEVICE_KEY, device, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    return device;
}
const char *arti_ime_set(void *raw, const char *text) {
    WKWebView *view = (__bridge WKWebView *)raw;
    NSResponder *responder = view.window.firstResponder;
    /* Only this WebView (or a view inside it) may receive the composition. */
    if (![responder isKindOfClass:[NSView class]] || ![(NSView *)responder isDescendantOf:view])
        return "The focused responder is not this WebView";
    if (![responder conformsToProtocol:@protocol(NSTextInputClient)])
        return "The focused WebView is not a native text input client";
    ArtiIme *device = device_for(view);
    device.target = (NSResponder<NSTextInputClient> *)responder;
    NSString *marked = [NSString stringWithUTF8String:text];
    [device.target setMarkedText:marked selectedRange:NSMakeRange(marked.length, 0)
               replacementRange:NSMakeRange(NSNotFound, 0)];
    device.active = YES;
    return NULL;
}
const char *arti_ime_commit(void *raw, const char *text) {
    ArtiIme *device = device_for((__bridge WKWebView *)raw);
    if (!device.active || !device.target) return "No native composition is active";
    [device.target insertText:[NSString stringWithUTF8String:text] replacementRange:[device.target markedRange]];
    [device.target unmarkText];
    device.active = NO;
    return NULL;
}
const char *arti_ime_cancel(void *raw) { return arti_ime_commit(raw, ""); }

/* WKWebView's responder editing actions, not constructed AppKit events. */
/* Sent to the WKWebView itself, never forwarded up the responder chain. */
const char *arti_edit_copy(void *raw) {
    WKWebView *view = (__bridge WKWebView *)raw;
    if (![view respondsToSelector:@selector(copy:)]) return "The WebView has no Copy editing action";
    [(id)view copy:nil];
    return NULL;
}
const char *arti_edit_paste(void *raw) {
    WKWebView *view = (__bridge WKWebView *)raw;
    if (![view respondsToSelector:@selector(paste:)]) return "The WebView has no Paste editing action";
    [(id)view paste:nil];
    return NULL;
}
