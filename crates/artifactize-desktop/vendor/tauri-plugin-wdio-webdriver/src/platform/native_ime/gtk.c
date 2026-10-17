/* Test input-method device. Composition goes through WebKit's native IME context,
 * never DOM event constructors or application commands. */
#include <webkit2/webkit2.h>
#include <gdk/gdkkeysyms.h>
#include <string.h>

typedef struct { WebKitInputMethodContext parent; gchar *text; gboolean active; } WdioIme;
typedef struct { WebKitInputMethodContextClass parent; } WdioImeClass;
G_DEFINE_TYPE(WdioIme, wdio_ime, WEBKIT_TYPE_INPUT_METHOD_CONTEXT)

static void preedit(WebKitInputMethodContext *context, gchar **text, GList **underlines, guint *cursor) {
    WdioIme *ime = (WdioIme *)context;
    *text = g_strdup(ime->text);
    *cursor = (guint)g_utf8_strlen(ime->text, -1);
    *underlines = NULL;
}
static gboolean filter_key(WebKitInputMethodContext *context, GdkEventKey *event) {
    WdioIme *ime = (WdioIme *)context;
    if (!ime->active || event->type != GDK_KEY_PRESS) return FALSE;
    /* Text keys belong to the input method. Chords pass through the native
     * keyboard path while preedit remains active, exercising the app's guard. */
    if (event->state & (GDK_CONTROL_MASK | GDK_META_MASK)) return FALSE;
    g_signal_emit_by_name(context, "preedit-changed");
    return TRUE;
}
static void finalize(GObject *object) {
    g_free(((WdioIme *)object)->text);
    G_OBJECT_CLASS(wdio_ime_parent_class)->finalize(object);
}
static void wdio_ime_class_init(WdioImeClass *klass) {
    WebKitInputMethodContextClass *context = WEBKIT_INPUT_METHOD_CONTEXT_CLASS(klass);
    context->get_preedit = preedit;
    context->filter_key_event = filter_key;
    G_OBJECT_CLASS(klass)->finalize = finalize;
}
static void wdio_ime_init(WdioIme *ime) { ime->text = g_strdup(""); ime->active = FALSE; }

typedef struct { WebKitInputMethodContext *original; WdioIme *ime; } Device;
static const char *DEVICE = "wdio-native-ime-device";
static void release_device(gpointer data) {
    Device *device = data;
    if (device->original) g_object_unref(device->original);
    g_object_unref(device->ime);
    g_free(device);
}
static Device *device_for(WebKitWebView *view) {
    Device *device = g_object_get_data(G_OBJECT(view), DEVICE);
    if (device) return device;
    device = g_new0(Device, 1);
    device->original = webkit_web_view_get_input_method_context(view);
    if (device->original) g_object_ref(device->original);
    device->ime = g_object_new(wdio_ime_get_type(), NULL);
    g_object_set_data_full(G_OBJECT(view), DEVICE, device, release_device);
    return device;
}

const char *wdio_ime_set(void *raw, const char *text) {
    WebKitWebView *view = WEBKIT_WEB_VIEW(raw);
    Device *device = device_for(view);
    gboolean started = !device->ime->active;
    webkit_web_view_set_input_method_context(view, WEBKIT_INPUT_METHOD_CONTEXT(device->ime));
    g_free(device->ime->text);
    device->ime->text = g_strdup(text);
    device->ime->active = TRUE;
    if (started) g_signal_emit_by_name(device->ime, "preedit-started");
    g_signal_emit_by_name(device->ime, "preedit-changed");
    return NULL;
}

const char *wdio_ime_key(void *raw, const char *key, int control, int meta, const char *preedit_text) {
    WebKitWebView *view = WEBKIT_WEB_VIEW(raw);
    Device *device = device_for(view);
    if (!device->ime->active) return "No native composition is active";
    if (preedit_text) { g_free(device->ime->text); device->ime->text = g_strdup(preedit_text); }
    guint keyval;
    const char *characters;
    if (!strcmp(key, "Escape")) { keyval = GDK_KEY_Escape; characters = "\033"; }
    else if (strlen(key) == 1 && g_ascii_isalpha(key[0])) { keyval = gdk_unicode_to_keyval(key[0]); characters = key; }
    else return "Native IME keys must be an ASCII letter or Escape";
    GtkWidget *widget = GTK_WIDGET(view);
    GdkWindow *window = gtk_widget_get_window(widget);
    if (!window) return "WebView has no native input window";
    GdkKeymapKey *entries = NULL;
    gint count = 0;
    if (!gdk_keymap_get_entries_for_keyval(gdk_keymap_get_for_display(gtk_widget_get_display(widget)), keyval, &entries, &count) || !count)
        return "No physical key is available for the native IME input";
    GdkKeymapKey physical = entries[0];
    g_free(entries);
    GdkEvent *event = gdk_event_new(GDK_KEY_PRESS);
    GdkDevice *keyboard = gdk_seat_get_keyboard(gdk_display_get_default_seat(gtk_widget_get_display(widget)));
    if (keyboard) { gdk_event_set_device(event, keyboard); gdk_event_set_source_device(event, keyboard); }
    event->key.window = g_object_ref(window);
    event->key.send_event = TRUE;
    event->key.time = GDK_CURRENT_TIME;
    event->key.state = (control ? GDK_CONTROL_MASK : 0) | (meta ? GDK_META_MASK : 0);
    event->key.keyval = keyval;
    event->key.hardware_keycode = physical.keycode;
    event->key.group = physical.group;
    event->key.string = g_strdup(characters);
    event->key.length = (gint)strlen(characters);
    gboolean handled = gtk_widget_event(widget, event);
    event->type = GDK_KEY_RELEASE;
    gtk_widget_event(widget, event);
    gdk_event_free(event);
    return handled ? NULL : "The native WebView did not handle the IME key";
}

const char *wdio_ime_commit(void *raw, const char *text) {
    WebKitWebView *view = WEBKIT_WEB_VIEW(raw);
    Device *device = device_for(view);
    g_signal_emit_by_name(device->ime, "committed", text);
    g_free(device->ime->text);
    device->ime->text = g_strdup("");
    g_signal_emit_by_name(device->ime, "preedit-changed");
    g_signal_emit_by_name(device->ime, "preedit-finished");
    device->ime->active = FALSE;
    webkit_web_view_set_input_method_context(view, device->original);
    return NULL;
}
const char *wdio_ime_cancel(void *raw) { return wdio_ime_commit(raw, ""); }
