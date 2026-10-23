//! A container whose natural width is `width` whatever its child asks for, and which shrinks down
//! to the child's minimum. `Clamp::new` also never makes the child wider than `width` (centered
//! when there is more room): the player bar's transport, the official client's fixed width on wide
//! windows yet usable on narrow ones. `Clamp::natural` lets the child fill any extra room: table
//! columns whose cells must shrink alike in every row, whatever their text.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Clamp {
        pub child: RefCell<Option<gtk::Widget>>,
        pub max_width: Cell<i32>,
        pub fill: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Clamp {
        const NAME: &'static str = "RtifyClamp";
        type Type = super::Clamp;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Clamp {
        fn dispose(&self) {
            if let Some(c) = self.child.take() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for Clamp {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.child.borrow().clone() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, _, _) = child.measure(orientation, for_size);
            if orientation == gtk::Orientation::Horizontal {
                (min, nat.max(self.max_width.get()).max(min), -1, -1)
            } else {
                (min, nat, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.child.borrow().as_ref() {
                let w = if self.fill.get() {
                    width
                } else {
                    width.min(self.max_width.get().max(child.measure(gtk::Orientation::Horizontal, -1).0))
                };
                let x = (width - w) / 2;
                child.size_allocate(&gtk::Allocation::new(x, 0, w, height), baseline);
            }
        }
    }
}

glib::wrapper! {
    pub struct Clamp(ObjectSubclass<imp::Clamp>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Clamp {
    pub fn new(child: &impl IsA<gtk::Widget>, max_width: i32) -> Self {
        let this: Self = glib::Object::new();
        child.set_parent(&this);
        *this.imp().child.borrow_mut() = Some(child.clone().upcast());
        this.imp().max_width.set(max_width);
        this
    }

    /// Natural width `width`, filling whatever it is given.
    pub fn natural(child: &impl IsA<gtk::Widget>, width: i32) -> Self {
        let this = Self::new(child, width);
        this.imp().fill.set(true);
        this.set_hexpand(child.as_ref().hexpands());
        this
    }
}
