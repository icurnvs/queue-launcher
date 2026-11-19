use super::*;
use block2::{Block, RcBlock};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::{
    define_class, msg_send,
    rc::{autoreleasepool, Retained, Weak},
    runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject},
    DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_app_kit::NSView;
use objc2_foundation::{
    NSArray, NSError, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL, NSUUID,
};
use objc2_foundation::{NSURLErrorCancelled, NSURLErrorDomain};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ptr::NonNull,
    time::Duration,
};

// The launcher is shared with Tauri's command dispatcher, while these
// AppKit objects are created, accessed, and released on the main thread.
thread_local! {
    static VIEWS: RefCell<HashMap<Uuid, NativeView>> = RefCell::default();
    static LAUNCHER_VIEW: RefCell<Option<Weak<NSView>>> = RefCell::default();
}

struct NativeView {
    view: Retained<WKWebView>,
    anchor: Retained<NSView>,
    delegate: Retained<NavigationDelegate>,
}

impl NativeView {
    fn viewport(&self) -> Result<NSRect> {
        let window = self
            .anchor
            .window()
            .ok_or("The launcher window is unavailable.")?;
        // contentLayoutRect is in window coordinates and excludes the title
        // bar. Convert it to the launcher view's coordinate system.
        Ok(self
            .anchor
            .convertRect_fromView(window.contentLayoutRect(), None))
    }
}

pub fn attach_launcher(chrome: &tauri::Webview) -> Result<()> {
    chrome.with_webview(|platform| {
        // Tauri supplies the live WKWebView on the main thread. Keep only a
        // weak anchor globally so closing the window releases its chrome.
        let view = unsafe { &*platform.inner().cast::<NSView>() };
        LAUNCHER_VIEW.with_borrow_mut(|anchor| *anchor = Some(Weak::from_retained(&view.retain())));
    })?;
    Ok(())
}

pub fn menu_position(window: &Window, x: f64, y: f64) -> Result<tauri::LogicalPosition<f64>> {
    let anchor = LAUNCHER_VIEW
        .with_borrow(|anchor| anchor.as_ref().and_then(Weak::load))
        .ok_or("The launcher webview is unavailable.")?;
    let parent = unsafe { &*window.ns_view()?.cast::<NSView>() };
    let native_window = anchor
        .window()
        .ok_or("The launcher window is unavailable.")?;
    let viewport = anchor.convertRect_fromView(native_window.contentLayoutRect(), None);
    let point = NSPoint::new(
        viewport.origin.x + x,
        viewport.origin.y
            + if anchor.isFlipped() {
                y
            } else {
                viewport.size.height - y
            },
    );
    let point = parent.convertPoint_fromView(point, Some(&anchor));
    Ok(tauri::LogicalPosition::new(
        point.x,
        parent.frame().size.height - point.y,
    ))
}

pub struct Browser(Uuid);

impl Browser {
    pub fn create(
        window: &Window,
        _root: &Path,
        profile: Uuid,
        instance: Uuid,
        original: &Url,
        start: &Url,
        demo: bool,
        events: &BrowserEvents,
    ) -> Result<Self> {
        let mtm = MainThreadMarker::new().ok_or("WebKit requires the main thread.")?;
        // Tauri owns the content view for the lifetime of the window.
        let parent = unsafe { &*window.ns_view()?.cast::<NSView>() };
        let anchor = LAUNCHER_VIEW
            .with_borrow(|anchor| anchor.as_ref().and_then(Weak::load))
            .ok_or("The launcher webview is unavailable.")?;
        autoreleasepool(|_| {
            let view = unsafe {
                let configuration = WKWebViewConfiguration::new(mtm);
                configuration.setWebsiteDataStore(&WKWebsiteDataStore::dataStoreForIdentifier(
                    &NSUUID::from_bytes(*profile.as_bytes()),
                    mtm,
                ));
                WKWebView::initWithFrame_configuration(
                    mtm.alloc(),
                    NSRect::new(NSPoint::ZERO, NSSize::new(1., 1.)),
                    &configuration,
                )
            };
            let delegate =
                NavigationDelegate::new(mtm, instance, original.clone(), demo, events.clone());
            view.setHidden(true);
            view.setClipsToBounds(true);
            unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
            parent.addSubview(&view);
            VIEWS.with_borrow_mut(|views| {
                views.insert(
                    instance,
                    NativeView {
                        view,
                        anchor,
                        delegate,
                    },
                )
            });
            let browser = Self(instance);
            browser.navigate(start.clone())?;
            Ok(browser)
        })
    }

    fn with_view<T>(&self, work: impl FnOnce(&NativeView) -> Result<T>) -> Result<T> {
        VIEWS.with_borrow(|views| work(views.get(&self.0).ok_or("WebKit session is closed.")?))
    }

    pub fn bounds(&self, layout: Layout) -> Result<()> {
        self.with_view(|native| {
            let parent = unsafe { native.view.superview() }.ok_or("WebKit session is detached.")?;
            let bounds = native.viewport()?;
            let y = if native.anchor.isFlipped() {
                layout.y
            } else {
                bounds.size.height - layout.y - layout.height
            };
            // WebKit's frame can extend behind the title bar. DOM coordinates
            // start inside the unobscured viewport, not at that frame's edge.
            let rect = NSRect::new(
                NSPoint::new(bounds.origin.x + layout.x, bounds.origin.y + y),
                NSSize::new(layout.width, layout.height),
            );
            native
                .view
                .setFrame(parent.convertRect_fromView(rect, Some(&native.anchor)));
            Ok(())
        })
    }

    #[cfg(debug_assertions)]
    pub(crate) fn viewport_size(&self) -> Result<(f64, f64)> {
        self.with_view(|native| {
            let rect = native.viewport()?;
            Ok((rect.size.width, rect.size.height))
        })
    }

    #[cfg(debug_assertions)]
    pub(crate) fn frame_in_launcher(&self) -> Result<Layout> {
        self.with_view(|native| {
            let parent = unsafe { native.view.superview() }.ok_or("session detached")?;
            let rect = native
                .anchor
                .convertRect_fromView(native.view.frame(), Some(&parent));
            let bounds = native.viewport()?;
            Ok(Layout {
                x: rect.origin.x - bounds.origin.x,
                y: if native.anchor.isFlipped() {
                    rect.origin.y - bounds.origin.y
                } else {
                    bounds.size.height - (rect.origin.y - bounds.origin.y) - rect.size.height
                },
                width: rect.size.width,
                height: rect.size.height,
                visible: !native.view.isHidden(),
            })
        })
    }

    pub fn visible(&self, visible: bool) -> Result<()> {
        self.with_view(|native| {
            native.view.setHidden(!visible);
            Ok(())
        })
    }

    pub fn navigate(&self, url: Url) -> Result<()> {
        self.with_view(|native| {
            let target = NSURL::URLWithString(&NSString::from_str(url.as_str()))
                .ok_or("Could not create a WebKit URL.")?;
            let navigation = unsafe {
                if url.scheme() == "file" {
                    native
                        .view
                        .loadFileURL_allowingReadAccessToURL(&target, &target)
                } else {
                    native
                        .view
                        .loadRequest(&NSURLRequest::requestWithURL(&target))
                }
            };
            native.delegate.started(navigation.as_deref());
            Ok(())
        })
    }

    pub fn url(&self) -> Result<Url> {
        self.with_view(|native| {
            unsafe { native.view.URL() }
                .and_then(|url| parse_url(&url))
                .ok_or_else(|| "WebKit has no current page.".into())
        })
    }

    pub fn back(&self) -> Result<()> {
        self.with_view(|native| {
            let navigation = unsafe { native.view.goBack() };
            native.delegate.started(navigation.as_deref());
            Ok(())
        })
    }

    pub fn reload(&self) -> Result<()> {
        self.with_view(|native| {
            let navigation = unsafe { native.view.reload() };
            native.delegate.started(navigation.as_deref());
            Ok(())
        })
    }

    pub fn can_go_back(&self) -> bool {
        self.with_view(|native| Ok(unsafe { native.view.canGoBack() }))
            .unwrap_or(false)
    }

    pub fn sample(&self, callback: impl Fn(String) + Send + 'static) -> Result<()> {
        self.evaluate(&format!("JSON.stringify({})", crate::READ_PAGE), callback)
    }

    pub(crate) fn evaluate(&self, script: &str, callback: impl Fn(String) + 'static) -> Result<()> {
        self.with_view(|native| {
            let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                if error.is_null() {
                    if let Some(text) =
                        unsafe { value.as_ref() }.and_then(AnyObject::downcast_ref::<NSString>)
                    {
                        callback(text.to_string());
                    }
                }
            });
            unsafe {
                native.view.evaluateJavaScript_completionHandler(
                    &NSString::from_str(script),
                    Some(&completion),
                );
            }
            Ok(())
        })
    }

    pub fn discard(self) -> Result<()> {
        drop(self);
        Ok(())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if MainThreadMarker::new().is_some() {
            autoreleasepool(|_| {
                if let Ok(Some(native)) = VIEWS.try_with(|views| views.borrow_mut().remove(&self.0))
                {
                    unsafe {
                        native.view.setNavigationDelegate(None);
                        native.view.stopLoading();
                    }
                    native.view.removeFromSuperview();
                }
            });
        }
    }
}

fn parse_url(url: &NSURL) -> Option<Url> {
    Url::parse(&url.absoluteString()?.to_string()).ok()
}

pub fn remove_store(id: Uuid, completed: impl Fn(bool) + Clone + Send + 'static) {
    // Detaching releases our ownership immediately. WebKit can still be
    // finishing work against the store, so retry briefly on the main queue.
    remove_store_after(id, &[0, 100, 250, 500, 1000, 2000], completed);
}

fn remove_store_after(
    id: Uuid,
    delays: &'static [u64],
    completed: impl Fn(bool) + Clone + Send + 'static,
) {
    let Some((&delay, remaining)) = delays.split_first() else {
        // A process may have exited after removing the store and before
        // committing the journal update. An absent identifier is clean.
        store_exists(id, move |exists| completed(!exists));
        return;
    };
    let when = DispatchTime::try_from(Duration::from_millis(delay)).expect("valid cleanup delay");
    let callback = completed.clone();
    if DispatchQueue::main()
        .after(when, move || {
            let mtm = MainThreadMarker::new().expect("main dispatch queue");
            let identifier = NSUUID::from_bytes(*id.as_bytes());
            let completion = RcBlock::new(move |error: *mut NSError| {
                if error.is_null() {
                    callback(true);
                } else {
                    remove_store_after(id, remaining, callback.clone());
                }
            });
            unsafe {
                WKWebsiteDataStore::removeDataStoreForIdentifier_completionHandler(
                    &identifier,
                    &completion,
                    mtm,
                )
            };
        })
        .is_err()
    {
        completed(false);
    }
}

pub fn store_exists(id: Uuid, completed: impl Fn(bool) + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        completed(true);
        return;
    };
    let identifier = NSUUID::from_bytes(*id.as_bytes());
    let callback = RcBlock::new(move |ids: NonNull<NSArray<NSUUID>>| {
        // WebKit's completion owns this non-null array for the call.
        completed(unsafe { ids.as_ref() }.containsObject(&identifier));
    });
    unsafe { WKWebsiteDataStore::fetchAllDataStoreIdentifiers(&callback, mtm) };
}

struct Navigation {
    original: Url,
    demo: bool,
    instance: Uuid,
    events: BrowserEvents,
    sequence: Cell<u64>,
    active: RefCell<Option<Retained<WKNavigation>>>,
    superseded: RefCell<Vec<Weak<WKNavigation>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Navigation]
    struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {}

    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide(
            &self,
            _view: &WKWebView,
            action: &WKNavigationAction,
            decision: &Block<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            if unsafe { action.shouldPerformDownload() } {
                decision.call((WKNavigationActionPolicy::Cancel,));
                return;
            }
            let state = self.ivars();
            let url = unsafe { action.request() }
                .URL()
                .and_then(|url| parse_url(&url));
            let allowed = url.filter(|url| allows(url, &state.original, state.demo));
            if unsafe { action.targetFrame() }.is_none() {
                if let Some(url) = allowed {
                    self.send(Event::Popup(url));
                }
                decision.call((WKNavigationActionPolicy::Cancel,));
            } else {
                decision.call((if allowed.is_some() {
                    WKNavigationActionPolicy::Allow
                } else {
                    WKNavigationActionPolicy::Cancel
                },));
            }
        }

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn provisional(&self, _view: &WKWebView, navigation: Option<&WKNavigation>) {
            self.started(navigation);
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _view: &WKWebView, navigation: Option<&WKNavigation>) {
            self.finished(navigation, None);
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn provisional_failed(
            &self,
            _view: &WKWebView,
            navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.failed(navigation, error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn committed_failed(
            &self,
            _view: &WKWebView,
            navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.failed(navigation, error);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn terminated(&self, _view: &WKWebView) {
            self.ivars().active.take();
            self.ivars().superseded.borrow_mut().clear();
            self.send(Event::Terminated(None));
        }
    }
);

impl NavigationDelegate {
    fn new(
        mtm: MainThreadMarker,
        instance: Uuid,
        original: Url,
        demo: bool,
        events: BrowserEvents,
    ) -> Retained<Self> {
        let delegate = mtm.alloc::<Self>().set_ivars(Navigation {
            original,
            demo,
            instance,
            events,
            sequence: Cell::new(0),
            active: RefCell::new(None),
            superseded: RefCell::default(),
        });
        unsafe { msg_send![super(delegate), init] }
    }

    fn send(&self, event: Event) {
        let _ = self.ivars().events.send((self.ivars().instance, event));
    }

    fn started(&self, navigation: Option<&WKNavigation>) {
        let Some(navigation) = navigation else { return };
        let state = self.ivars();
        let mut active = state.active.borrow_mut();
        if active
            .as_deref()
            .is_some_and(|current| std::ptr::eq(current, navigation))
        {
            return;
        }
        // WebKit can deliver callbacks from a superseded load after the
        // application has already requested its replacement.
        let mut superseded = state.superseded.borrow_mut();
        superseded.retain(|old| old.load().is_some());
        if superseded.iter().any(|old| {
            old.load()
                .is_some_and(|old| std::ptr::eq(&*old, navigation))
        }) {
            return;
        }
        if let Some(old) = active.replace(navigation.retain()) {
            superseded.push(Weak::from_retained(&old));
        }
        let id = state.sequence.get().wrapping_add(1);
        state.sequence.set(id);
        self.send(Event::Starting(id));
    }

    fn finished(&self, navigation: Option<&WKNavigation>, error: Option<i32>) {
        let state = self.ivars();
        let mut active = state.active.borrow_mut();
        if let (Some(current), Some(completed)) = (active.as_deref(), navigation) {
            if std::ptr::eq(current, completed) {
                active.take();
                self.send(Event::Finished(state.sequence.get(), error));
            }
        }
    }

    fn failed(&self, navigation: Option<&WKNavigation>, error: &NSError) {
        let code = error.code();
        let cancelled =
            code == NSURLErrorCancelled && &*error.domain() == unsafe { NSURLErrorDomain };
        self.finished(navigation, (!cancelled).then_some(code as i32));
    }
}
