use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use super::context::TuiViewContext;
use super::TuiView;
use crate::core::view::AnyViewHandle;
use crate::core::RefCounts;
use crate::{AppContext, EntityId, WindowId};

/// A strong reference to a particular [`TuiView`] instance, mirroring the GUI
/// [`ViewHandle`](crate::ViewHandle).
pub struct TuiViewHandle<T> {
    window_id: WindowId,
    view_id: EntityId,
    view_type: PhantomData<T>,
    ref_counts: Weak<Mutex<RefCounts>>,
}

impl<T: TuiView> TuiViewHandle<T> {
    pub(in crate::core) fn new(
        window_id: WindowId,
        view_id: EntityId,
        ref_counts: &Arc<Mutex<RefCounts>>,
    ) -> Self {
        ref_counts.lock().inc_entity(view_id);
        Self {
            window_id,
            view_id,
            view_type: PhantomData,
            ref_counts: Arc::downgrade(ref_counts),
        }
    }

    pub fn downgrade(&self) -> WeakTuiViewHandle<T> {
        WeakTuiViewHandle::new(self.view_id)
    }

    /// Returns the current window this view belongs to.
    pub fn window_id(&self, app: &AppContext) -> WindowId {
        app.view_to_window
            .get(&self.view_id)
            .copied()
            .unwrap_or(self.window_id)
    }

    pub fn id(&self) -> EntityId {
        self.view_id
    }

    /// Convert a handle to a reference of the underlying [`TuiView`].
    pub fn as_ref<'a, A: TuiViewAsRef>(&self, app: &'a A) -> &'a T {
        app.tui_view(self)
    }

    /// Try to convert a handle to a reference of the underlying [`TuiView`].
    /// Returns `None` if the view is currently borrowed (circular reference).
    pub fn try_as_ref<'a, A: TuiViewAsRef>(&self, app: &'a A) -> Option<&'a T> {
        app.try_tui_view(self)
    }

    /// Reads a value out of the underlying view.
    pub fn read<A, F, S>(&self, app: &A, read: F) -> S
    where
        A: TuiReadView,
        F: FnOnce(&T, &AppContext) -> S,
    {
        app.read_tui_view(self, read)
    }

    /// Updates a value within the underlying view.
    pub fn update<A, F, S>(&self, app: &mut A, update: F) -> S
    where
        A: TuiUpdateView,
        F: FnOnce(&mut T, &mut TuiViewContext<T>) -> S,
    {
        app.update_tui_view(self, update)
    }

    pub fn is_focused(&self, app: &AppContext) -> bool {
        app.focused_view_id(self.window_id(app)) == Some(self.view_id)
    }
}

impl<T> Clone for TuiViewHandle<T> {
    fn clone(&self) -> Self {
        if let Some(ref_counts) = self.ref_counts.upgrade() {
            ref_counts.lock().inc_entity(self.view_id);
        }

        Self {
            window_id: self.window_id,
            view_id: self.view_id,
            view_type: PhantomData,
            ref_counts: self.ref_counts.clone(),
        }
    }
}

impl<T> PartialEq for TuiViewHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.window_id == other.window_id && self.view_id == other.view_id
    }
}

impl<T> Eq for TuiViewHandle<T> {}

impl<T> Debug for TuiViewHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!("TuiViewHandle<{}>", core::any::type_name::<T>()))
            .field("window_id", &self.window_id)
            .field("view_id", &self.view_id)
            .finish()
    }
}

impl<T> Drop for TuiViewHandle<T> {
    fn drop(&mut self) {
        if let Some(ref_counts) = self.ref_counts.upgrade() {
            ref_counts.lock().dec_view(self.window_id, self.view_id);
        }
    }
}

unsafe impl<T> Send for TuiViewHandle<T> {}
unsafe impl<T> Sync for TuiViewHandle<T> {}

impl<T: TuiView> From<&TuiViewHandle<T>> for AnyViewHandle {
    fn from(handle: &TuiViewHandle<T>) -> Self {
        AnyViewHandle::for_view::<T>(handle.window_id, handle.view_id, &handle.ref_counts)
    }
}

impl<T: TuiView> From<TuiViewHandle<T>> for AnyViewHandle {
    fn from(handle: TuiViewHandle<T>) -> Self {
        (&handle).into()
    }
}

/// A weak reference to a particular [`TuiView`] instance, mirroring the GUI
/// [`WeakViewHandle`](crate::WeakViewHandle).
pub struct WeakTuiViewHandle<T> {
    view_id: EntityId,
    view_type: PhantomData<T>,
}

impl<T: TuiView> WeakTuiViewHandle<T> {
    pub(super) fn new(view_id: EntityId) -> Self {
        Self {
            view_id,
            view_type: PhantomData,
        }
    }

    pub fn upgrade(&self, app: &AppContext) -> Option<TuiViewHandle<T>> {
        let window_id = app.view_to_window.get(&self.view_id).copied()?;

        if app
            .windows
            .get(&window_id)
            .and_then(|w| w.views.get(&self.view_id))
            .is_some()
            && !app.ref_counts.lock().is_view_dropped(self.view_id)
        {
            Some(TuiViewHandle::new(window_id, self.view_id, &app.ref_counts))
        } else {
            None
        }
    }

    pub fn id(&self) -> EntityId {
        self.view_id
    }

    pub fn window_id(&self, app: &AppContext) -> Option<WindowId> {
        app.view_to_window.get(&self.view_id).copied()
    }
}

impl<T> Clone for WeakTuiViewHandle<T> {
    fn clone(&self) -> Self {
        Self {
            view_id: self.view_id,
            view_type: PhantomData,
        }
    }
}

impl<T> Debug for WeakTuiViewHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(&format!(
            "WeakTuiViewHandle<{}>",
            core::any::type_name::<T>()
        ))
        .field("view_id", &self.view_id)
        .finish()
    }
}

unsafe impl<T> Send for WeakTuiViewHandle<T> {}
unsafe impl<T> Sync for WeakTuiViewHandle<T> {}

pub trait TuiViewAsRef {
    fn tui_view<T: TuiView>(&self, handle: &TuiViewHandle<T>) -> &T;

    /// Try to get a reference to the view. Returns `None` if the view is
    /// currently borrowed (e.g., during a circular reference scenario).
    fn try_tui_view<T: TuiView>(&self, handle: &TuiViewHandle<T>) -> Option<&T>;
}

pub trait TuiReadView: TuiViewAsRef {
    fn read_tui_view<T, F, S>(&self, handle: &TuiViewHandle<T>, read: F) -> S
    where
        T: TuiView,
        F: FnOnce(&T, &AppContext) -> S;
}

pub trait TuiUpdateView: TuiReadView {
    fn update_tui_view<T, F, S>(&mut self, handle: &TuiViewHandle<T>, update: F) -> S
    where
        T: TuiView,
        F: FnOnce(&mut T, &mut TuiViewContext<T>) -> S;
}
