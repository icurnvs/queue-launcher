use crate::{Result, Window, HEIGHT, TOOLBAR, WIDTH};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use webview2_com::{
    take_pwstr, ClearBrowsingDataCompletedHandler, ExecuteScriptCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Profile, ICoreWebView2Profile2, ICoreWebView2Profile8, ICoreWebView2_13,
    },
};
use windows_core::{Interface, PWSTR};
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

// Wry and WebView2 controllers belong to the window's UI thread.
thread_local! {
    static VIEWS: RefCell<HashMap<Uuid, WebView>> = RefCell::default();
}

fn build(
    window: &Window,
    root: &Path,
    id: Uuid,
    fixture: Option<&str>,
    verifying: bool,
) -> Result<WebView> {
    build_named(window, root, id, move |mut builder| {
        builder = builder
            .with_bounds(wry::Rect {
                position: wry::dpi::LogicalPosition::new(0., TOOLBAR).into(),
                size: wry::dpi::LogicalSize::new(WIDTH, HEIGHT - TOOLBAR).into(),
            })
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny);
        if let Some(url) = fixture {
            let allowed = url.to_owned();
            builder = builder
                .with_url(url)
                .with_navigation_handler(move |target| target == allowed);
            if verifying {
                builder =
                    builder.with_initialization_script(format!("window.fixtureSession = '{id}';"));
            }
        } else {
            builder = builder.with_navigation_handler(|_| false);
        }
        builder
    })
}

pub fn build_named(
    window: &Window,
    root: &Path,
    id: Uuid,
    configure: impl for<'a> FnOnce(WebViewBuilder<'a>) -> WebViewBuilder<'a>,
) -> Result<WebView> {
    let name = id.simple().to_string();
    let mut context = wry::WebContext::new(Some(root.join("webview2")));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_profile_name(&name)
        .with_visible(false)
        .with_devtools(false);
    let builder = configure(builder);
    let view = builder.build_as_child(window)?;
    let mut profile_name = PWSTR::null();
    unsafe { profile(&view)?.ProfileName(&mut profile_name)? };
    if take_pwstr(profile_name) != name {
        return Err("WebView2 did not create the requested named profile".into());
    }
    Ok(view)
}

pub fn create(window: &Window, root: &Path, id: Uuid, url: &str, verifying: bool) -> Result<()> {
    let view = build(window, root, id, Some(url), verifying)?;
    register(id, view);
    Ok(())
}

pub fn register(id: Uuid, view: WebView) {
    VIEWS.with(|views| views.borrow_mut().insert(id, view));
}

pub fn with_view<T>(id: Uuid, f: impl FnOnce(&WebView) -> Result<T>) -> Result<T> {
    VIEWS.with(|views| {
        let views = views.borrow();
        f(views.get(&id).ok_or("missing native session view")?)
    })
}

pub fn set_visible(id: Uuid, visible: bool) -> Result<()> {
    with_view(id, |view| Ok(view.set_visible(visible)?))
}

pub fn eval(id: Uuid, script: &str) -> Result<()> {
    with_view(id, |view| Ok(view.evaluate_script(script)?))
}

pub fn discard(id: Uuid) {
    let view = VIEWS.with(|views| views.borrow_mut().remove(&id));
    drop(view);
}

fn profile(view: &WebView) -> Result<ICoreWebView2Profile> {
    // Called on the thread that created the controller.
    unsafe {
        Ok(view
            .controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?)
    }
}

pub fn profile_path(id: Uuid) -> Result<PathBuf> {
    with_view(id, |view| {
        let mut path = PWSTR::null();
        unsafe { profile(view)?.ProfilePath(&mut path)? };
        Ok(take_pwstr(path).into())
    })
}

fn delete(view: WebView, verify_clear: bool) -> Result<()> {
    let profile = profile(&view)?;
    let core = unsafe { view.controller().CoreWebView2()? };
    unsafe { core.Stop()? };
    let clearing = profile.cast::<ICoreWebView2Profile2>()?;
    let deleting = profile.cast::<ICoreWebView2Profile8>()?;
    ClearBrowsingDataCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            clearing.ClearBrowsingDataAll(&handler).map_err(Into::into)
        }),
        Box::new(|result| result),
    )?;
    if verify_clear {
        ExecuteScriptCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                core.ExecuteScript(
                    windows_core::w!("(localStorage.getItem('local-queue-id') === null && document.cookie === '')"),
                    &handler,
                ).map_err(Into::into)
            }),
            Box::new(|status, value| {
                status?;
                if value == "true" {
                    Ok(())
                } else {
                    Err(windows_core::Error::new(
                        windows_core::HRESULT(0x80004005_u32 as i32),
                        "named profile still has fixture data after clearing",
                    ))
                }
            }),
        )?;
    }
    unsafe { deleting.Delete()? };
    Ok(())
}

pub fn close(id: Uuid, verify_clear: bool) -> Result<()> {
    let view = VIEWS.with(|views| views.borrow_mut().remove(&id));
    delete(view.ok_or("missing named profile to delete")?, verify_clear)
}

pub fn retry_delete(window: &Window, root: &Path, id: Uuid) -> Result<()> {
    match build(window, root, id, None, false) {
        Ok(view) => delete(view, false),
        Err(error) if deletion_pending(error.as_ref()) => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn deletion_pending(error: &(dyn std::error::Error + 'static)) -> bool {
    matches!(
        error.downcast_ref::<wry::Error>(),
        Some(wry::Error::WebView2Error(webview2_com::Error::WindowsError(error)))
            if error.code().0 as u32 == 0x8007012F // HRESULT_FROM_WIN32(ERROR_DELETE_PENDING)
    )
}
