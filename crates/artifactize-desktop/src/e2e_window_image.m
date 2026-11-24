// The test app snapshots its own webview. Unlike desktop capture this needs no Screen
// Recording permission, and it never observes another window or sends input.
#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>

void arti_e2e_window_image(void *raw, const char *destination) {
    WKWebView *view = (__bridge WKWebView *)raw;
    NSString *path = [NSString stringWithUTF8String:destination];
    if (path == nil || view.window == nil || !view.window.visible) return;
    [view takeSnapshotWithConfiguration:nil completionHandler:^(NSImage *image, NSError *error) {
        if (error != nil || image == nil) return;
        NSData *tiff = image.TIFFRepresentation;
        if (tiff == nil) return;
        NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:tiff];
        NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        [png writeToFile:path options:NSDataWritingAtomic error:NULL];
    }];
}
