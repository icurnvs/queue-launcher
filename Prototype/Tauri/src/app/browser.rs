use crate::{Result, Window};
use serde::Deserialize;
use std::{path::Path, sync::mpsc::Sender};
use tauri::{LogicalPosition, LogicalSize, Url};
use uuid::Uuid;

#[derive(Copy, Clone, Deserialize, Default)]
pub struct Layout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
}

impl Layout {
    pub fn position(self) -> LogicalPosition<f64> {
        LogicalPosition::new(self.x, self.y)
    }
    pub fn size(self) -> LogicalSize<f64> {
        LogicalSize::new(self.width, self.height)
    }
}

pub enum Event {
    Starting(u64),
    Finished(u64, Option<i32>),
    #[cfg(target_os = "windows")]
    Terminated(i32),
    #[cfg(target_os = "macos")]
    Removed(usize, bool),
    Popup(Url),
    Sample(u64, String),
}

pub type BrowserEvents = Sender<(Uuid, Event)>;

pub fn destination(text: &str) -> Result<Url> {
    let url =
        Url::parse(text.trim()).map_err(|_| "Enter a valid HTTP, HTTPS, or local file URL.")?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Ok(url),
        "file"
            if url.host_str().is_none() && url.to_file_path().is_ok_and(|path| path.is_file()) =>
        {
            Ok(url)
        }
        _ => Err("Enter a valid HTTP, HTTPS, or local file URL.".into()),
    }
}

pub fn allows(target: &Url, original: &Url, demo: bool) -> bool {
    if demo {
        return target == original;
    }
    matches!(target.scheme(), "http" | "https")
        || (original.scheme() == "file" && target == original)
}

#[cfg(target_os = "windows")]
pub use win::Browser;

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use crate::named_profiles;
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2, ICoreWebView2NavigationCompletedEventArgs2,
            COREWEBVIEW2_PROCESS_FAILED_KIND,
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, COREWEBVIEW2_WEB_ERROR_STATUS,
            COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
        },
        NavigationCompletedEventHandler, NavigationStartingEventHandler, ProcessFailedEventHandler,
    };
    use windows_core::{Interface, BOOL};
    use wry::WebViewExtWindows;

    pub struct Browser(Uuid);

    impl Browser {
        pub fn create(
            window: &Window,
            root: &Path,
            profile: Uuid,
            instance: Uuid,
            original: &Url,
            start: &Url,
            demo: bool,
            events: &BrowserEvents,
        ) -> Result<Self> {
            let allowed = original.clone();
            let popup = events.clone();
            let view = named_profiles::build_named(window, root, profile, move |builder| {
                builder
                    .with_navigation_handler(move |target| {
                        Url::parse(&target).is_ok_and(|url| allows(&url, &allowed, demo))
                    })
                    .with_new_window_req_handler(move |target, _| {
                        if let Ok(url) = Url::parse(&target) {
                            let _ = popup.send((instance, Event::Popup(url)));
                        }
                        wry::NewWindowResponse::Deny
                    })
            })?;
            let core = unsafe { view.controller().CoreWebView2()? };
            install_events(&core, instance, events)?;
            named_profiles::register(profile, view);
            let browser = Self(profile);
            if let Err(error) = browser.navigate(start.clone()) {
                browser.discard()?;
                return Err(error);
            }
            Ok(browser)
        }

        fn core<T>(&self, work: impl FnOnce(ICoreWebView2) -> Result<T>) -> Result<T> {
            named_profiles::with_view(self.0, |view| {
                work(unsafe { view.controller().CoreWebView2()? })
            })
        }

        pub fn bounds(&self, layout: Layout) -> Result<()> {
            named_profiles::with_view(self.0, |view| {
                Ok(view.set_bounds(wry::Rect {
                    position: layout.position().into(),
                    size: layout.size().into(),
                })?)
            })
        }

        pub fn visible(&self, visible: bool) -> Result<()> {
            named_profiles::set_visible(self.0, visible)
        }
        pub fn navigate(&self, url: Url) -> Result<()> {
            named_profiles::with_view(self.0, |view| Ok(view.load_url(url.as_str())?))
        }
        pub fn url(&self) -> Result<Url> {
            named_profiles::with_view(self.0, |view| Ok(Url::parse(&view.url()?)?))
        }
        pub fn back(&self) -> Result<()> {
            self.core(|core| Ok(unsafe { core.GoBack()? }))
        }
        pub fn reload(&self) -> Result<()> {
            self.core(|core| Ok(unsafe { core.Reload()? }))
        }
        pub fn can_go_back(&self) -> bool {
            self.core(|core| {
                let mut value = BOOL(0);
                unsafe { core.CanGoBack(&mut value)? };
                Ok(value.as_bool())
            })
            .unwrap_or(false)
        }
        pub fn sample(&self, callback: impl Fn(String) + Send + 'static) -> Result<()> {
            named_profiles::with_view(self.0, |view| {
                Ok(view.evaluate_script_with_callback(crate::READ_PAGE, callback)?)
            })
        }
        pub fn discard(self) -> Result<()> {
            named_profiles::discard(self.0);
            Ok(())
        }
        pub fn delete(self) -> Result<()> {
            named_profiles::close(self.0, false)
        }
    }

    fn install_events(core: &ICoreWebView2, instance: Uuid, events: &BrowserEvents) -> Result<()> {
        let start = events.clone();
        let end = events.clone();
        let failure = events.clone();
        let mut token = 0;
        unsafe {
            core.Settings()?.SetIsWebMessageEnabled(false)?;
            core.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut canceled = BOOL(0);
                        args.Cancel(&mut canceled)?;
                        if !canceled.as_bool() {
                            let mut id = 0;
                            args.NavigationId(&mut id)?;
                            let _ = start.send((instance, Event::Starting(id)));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            core.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut id = 0;
                        let mut success = BOOL(0);
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                        args.NavigationId(&mut id)?;
                        args.IsSuccess(&mut success)?;
                        args.WebErrorStatus(&mut status)?;
                        let mut http_status = 0;
                        if let Ok(args2) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>()
                        {
                            args2.HttpStatusCode(&mut http_status)?;
                        }
                        let is_error = !success.as_bool()
                            && status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED
                            && http_status < 400;
                        let _ =
                            end.send((instance, Event::Finished(id, is_error.then_some(status.0))));
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            core.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&mut kind)?;
                        if matches!(
                            kind,
                            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED
                                | COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                        ) {
                            let _ = failure.send((instance, Event::Terminated(kind.0)));
                        }
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub struct Browser(tauri::Webview);

#[cfg(target_os = "macos")]
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
        use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
        let allowed = original.clone();
        let events = events.clone();
        let popup = events.clone();
        let builder = WebviewBuilder::new(
            format!("session-{instance}"),
            tauri::WebviewUrl::External(start.clone()),
        )
        .data_store_identifier(*profile.as_bytes())
        .devtools(false)
        .on_navigation(move |target| allows(target, &allowed, demo))
        .on_new_window(move |url, _| {
            let _ = popup.send((instance, Event::Popup(url)));
            NewWindowResponse::Deny
        })
        .on_page_load(move |_, payload| {
            let event = match payload.event() {
                PageLoadEvent::Started => Event::Starting(0),
                PageLoadEvent::Finished => Event::Finished(0, None),
            };
            let _ = events.send((instance, event));
        });
        let view = window.add_child(
            builder,
            LogicalPosition::new(0., 0.),
            LogicalSize::new(1., 1.),
        )?;
        view.hide()?;
        Ok(Self(view))
    }

    pub fn bounds(&self, layout: Layout) -> Result<()> {
        Ok(self.0.set_bounds(tauri::Rect {
            position: layout.position().into(),
            size: layout.size().into(),
        })?)
    }
    pub fn visible(&self, visible: bool) -> Result<()> {
        Ok(if visible {
            self.0.show()?
        } else {
            self.0.hide()?
        })
    }
    pub fn navigate(&self, url: Url) -> Result<()> {
        Ok(self.0.navigate(url)?)
    }
    pub fn url(&self) -> Result<Url> {
        Ok(self.0.url()?)
    }
    pub fn back(&self) -> Result<()> {
        Ok(self.0.eval("history.back()")?)
    }
    pub fn reload(&self) -> Result<()> {
        Ok(self.0.reload()?)
    }
    pub fn can_go_back(&self) -> bool {
        false
    }
    pub fn sample(&self, callback: impl Fn(String) + Send + 'static) -> Result<()> {
        Ok(self.0.eval_with_callback(crate::READ_PAGE, callback)?)
    }
    pub fn discard(self) -> Result<()> {
        Ok(self.0.close()?)
    }
}
