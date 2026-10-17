/* Engine-native input method for the evaluated E2E device; no keyboard events. */
#include <webkit2/webkit2.h>

typedef struct { WebKitInputMethodContext parent; gchar *text; gboolean active; } ArtiIme;
typedef struct { WebKitInputMethodContextClass parent; } ArtiImeClass;
G_DEFINE_TYPE(ArtiIme, arti_ime, WEBKIT_TYPE_INPUT_METHOD_CONTEXT)

static void preedit(WebKitInputMethodContext *context, gchar **text, GList **underlines, guint *cursor) {
    ArtiIme *ime = (ArtiIme *)context;
    *text = g_strdup(ime->text);
    *cursor = (guint)g_utf8_strlen(ime->text, -1);
    *underlines = NULL;
}
static void finalize(GObject *object) {
    g_free(((ArtiIme *)object)->text);
    G_OBJECT_CLASS(arti_ime_parent_class)->finalize(object);
}
static void arti_ime_class_init(ArtiImeClass *klass) {
    WEBKIT_INPUT_METHOD_CONTEXT_CLASS(klass)->get_preedit = preedit;
    G_OBJECT_CLASS(klass)->finalize = finalize;
}
static void arti_ime_init(ArtiIme *ime) { ime->text = g_strdup(""); ime->active = FALSE; }

typedef struct { WebKitInputMethodContext *original; ArtiIme *ime; } Device;
static const char *DEVICE = "artifactize-e2e-input";
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
    device->ime = g_object_new(arti_ime_get_type(), NULL);
    g_object_set_data_full(G_OBJECT(view), DEVICE, device, release_device);
    return device;
}
const char *arti_ime_set(void *raw, const char *text) {
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
const char *arti_ime_commit(void *raw, const char *text) {
    WebKitWebView *view = WEBKIT_WEB_VIEW(raw);
    Device *device = device_for(view);
    if (!device->ime->active) return "No native composition is active";
    g_signal_emit_by_name(device->ime, "committed", text);
    g_free(device->ime->text);
    device->ime->text = g_strdup("");
    g_signal_emit_by_name(device->ime, "preedit-changed");
    g_signal_emit_by_name(device->ime, "preedit-finished");
    device->ime->active = FALSE;
    webkit_web_view_set_input_method_context(view, device->original);
    return NULL;
}
const char *arti_ime_cancel(void *raw) { return arti_ime_commit(raw, ""); }
