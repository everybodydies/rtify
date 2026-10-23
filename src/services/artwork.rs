//! Cover art: downloaded once on the Tokio runtime, decoded into GDK textures and kept in a small
//! in-memory LRU so the album-art palette never re-downloads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};

use crate::runtime;
use crate::theme::album_palette;
use crate::theme::contrast::Rgb;

type Callback = Box<dyn FnOnce(Option<gdk::Texture>)>;

#[derive(Default)]
pub struct Artwork {
    cache: RefCell<HashMap<String, gdk::Texture>>,
    order: RefCell<Vec<String>>,
    pending: RefCell<HashMap<String, Vec<Callback>>>,
    accents: RefCell<HashMap<String, Option<Rgb>>>,
}

const CAPACITY: usize = 64;

impl Artwork {
    pub fn load(self: &Rc<Self>, http: &reqwest::Client, url: &str, cb: impl FnOnce(Option<gdk::Texture>) + 'static) {
        if url.is_empty() {
            return cb(None);
        }
        if let Some(t) = self.cache.borrow().get(url) {
            return cb(Some(t.clone()));
        }
        let mut pending = self.pending.borrow_mut();
        if let Some(waiters) = pending.get_mut(url) {
            waiters.push(Box::new(cb));
            return;
        }
        pending.insert(url.to_owned(), vec![Box::new(cb)]);
        drop(pending);
        let http = http.clone();
        let owned = url.to_owned();
        let this = Rc::clone(self);
        runtime::spawn_then(
            async move {
                let resp = http.get(&owned).send().await.and_then(|r| r.error_for_status());
                match resp {
                    Ok(r) => r.bytes().await.ok().map(|b| b.to_vec()),
                    Err(_) => None,
                }
            },
            {
                let url = url.to_owned();
                move |bytes| {
                    let texture = bytes.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from_owned(b)).ok());
                    if let Some(t) = &texture {
                        this.insert(&url, t.clone());
                    }
                    for waiter in this.pending.borrow_mut().remove(&url).unwrap_or_default() {
                        waiter(texture.clone());
                    }
                }
            },
        );
    }

    fn insert(&self, url: &str, texture: gdk::Texture) {
        let mut order = self.order.borrow_mut();
        order.retain(|u| u != url);
        order.push(url.to_owned());
        self.cache.borrow_mut().insert(url.to_owned(), texture);
        while order.len() > CAPACITY {
            let old = order.remove(0);
            self.cache.borrow_mut().remove(&old);
        }
    }

    /// Accent color of the artwork at `url`, computed off the main thread and memoised.
    pub fn accent(self: &Rc<Self>, http: &reqwest::Client, url: &str, cb: impl FnOnce(Option<Rgb>) + 'static) {
        if let Some(a) = self.accents.borrow().get(url) {
            return cb(*a);
        }
        let this = Rc::clone(self);
        let key = url.to_owned();
        self.load(http, url, move |texture| {
            let Some(texture) = texture else { return cb(None) };
            let mut d = gdk::TextureDownloader::new(&texture);
            d.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (bytes, stride) = d.download_bytes();
            let (w, h) = (texture.width() as usize, texture.height() as usize);
            glib::spawn_future_local(async move {
                let accent = gtk::gio::spawn_blocking(move || album_palette::dominant_accent(&bytes, w, h, stride, 4))
                    .await
                    .ok()
                    .flatten();
                this.accents.borrow_mut().insert(key, accent);
                cb(accent);
            });
        });
    }
}
