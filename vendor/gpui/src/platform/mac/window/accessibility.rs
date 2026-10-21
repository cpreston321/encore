//! NSAccessibility for a GPUI window: the view's children are `NSAccessibilityElement`s,
//! one per accessible element of the last frame, answering from that frame's nodes.

use super::*;
use crate::{AccessibilityNode, AccessibilityNodeId, AccessibilityRequest, Role};
use collections::{FxHashMap, FxHashSet};

const NODE_IVAR: &str = "gpuiNode";

static mut ELEMENT_CLASS: *const Class = ptr::null();

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSAccessibilityPostNotification(element: id, notification: id);
    fn NSAccessibilityRoleDescription(role: id, subrole: id) -> id;
}

struct ElementData {
    state: Weak<Mutex<MacWindowState>>,
    id: AccessibilityNodeId,
}

#[derive(Default)]
pub(super) struct AccessibilityState {
    pub(super) active: bool,
    pub(super) request_callback: Option<Box<dyn FnMut(AccessibilityRequest)>>,
    nodes: Vec<AccessibilityNode>,
    index: FxHashMap<AccessibilityNodeId, usize>,
    children: FxHashMap<AccessibilityNodeId, Vec<AccessibilityNodeId>>,
    roots: Vec<AccessibilityNodeId>,
    /// Retained; kept from frame to frame so VoiceOver's cursor stays on the same object.
    elements: FxHashMap<AccessibilityNodeId, id>,
}

impl Drop for AccessibilityState {
    fn drop(&mut self) {
        for element in self.elements.drain().map(|(_, e)| e) {
            unsafe {
                let _: () = msg_send![element, release];
            }
        }
    }
}

impl AccessibilityState {
    fn node(&self, id: AccessibilityNodeId) -> Option<&AccessibilityNode> {
        self.index.get(&id).map(|&ix| &self.nodes[ix])
    }

    fn element(&self, id: AccessibilityNodeId) -> id {
        self.elements.get(&id).copied().unwrap_or(nil)
    }

    fn array(&self, ids: &[AccessibilityNodeId]) -> id {
        let elements: Vec<id> = ids
            .iter()
            .filter_map(|id| self.elements.get(id).copied())
            .collect();
        unsafe { NSArray::arrayWithObjects(nil, &elements) }
    }

    /// Replaces the nodes and returns the notifications to post, with retained elements,
    /// once the window state is unlocked.
    fn update(
        &mut self,
        nodes: &[AccessibilityNode],
        state: &Weak<Mutex<MacWindowState>>,
        native_window: id,
    ) -> Vec<(id, &'static str)> {
        let mut notes = Vec::new();
        let mut layout_changed = false;
        let mut focused = None;

        let mut index = FxHashMap::default();
        let mut children: FxHashMap<AccessibilityNodeId, Vec<AccessibilityNodeId>> =
            FxHashMap::default();
        let mut roots = Vec::new();
        for (ix, node) in nodes.iter().enumerate() {
            index.insert(node.id, ix);
        }
        for node in nodes {
            match node.parent.filter(|p| index.contains_key(p)) {
                Some(parent) => children.entry(parent).or_default().push(node.id),
                None => roots.push(node.id),
            }
            match self.node(node.id) {
                None => {
                    layout_changed = true;
                    let element = new_element(state.clone(), node.id);
                    self.elements.insert(node.id, element);
                }
                Some(old) => {
                    if old.parent != node.parent {
                        layout_changed = true;
                    }
                    let element = self.element(node.id);
                    if old.value != node.value || old.toggled != node.toggled {
                        notes.push((retain(element), "AXValueChanged"));
                    }
                    if old.label != node.label {
                        notes.push((retain(element), "AXTitleChanged"));
                    }
                    if old.selected != node.selected && node.selected == Some(true) {
                        notes.push((retain(element), "AXSelectedChildrenChanged"));
                    }
                    if node.focused && !old.focused {
                        focused = Some(node.id);
                    }
                }
            }
            if node.focused && self.node(node.id).is_none() {
                focused = Some(node.id);
            }
        }

        let live: FxHashSet<AccessibilityNodeId> = index.keys().copied().collect();
        let gone: Vec<AccessibilityNodeId> = self
            .elements
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(element) = self.elements.remove(&id) {
                layout_changed = true;
                notes.push((element, "AXUIElementDestroyed"));
            }
        }

        self.nodes = nodes.to_vec();
        self.index = index;
        self.children = children;
        self.roots = roots;

        if let Some(id) = focused {
            notes.push((retain(self.element(id)), "AXFocusedUIElementChanged"));
        }
        if layout_changed {
            notes.push((retain(native_window), "AXLayoutChanged"));
        }
        notes
    }

    /// The node's frame in screen coordinates, which AppKit measures from the bottom left.
    fn screen_frame(&self, node: &AccessibilityNode, window: &MacWindowState) -> NSRect {
        unsafe {
            let view = window.native_view.as_ptr() as id;
            let view_height = NSView::frame(view).size.height;
            let b = node.bounds;
            let in_view = NSRect::new(
                NSPoint::new(
                    b.origin.x.0 as f64,
                    view_height - b.origin.y.0 as f64 - b.size.height.0 as f64,
                ),
                NSSize::new(b.size.width.0 as f64, b.size.height.0 as f64),
            );
            let in_window: NSRect = msg_send![view, convertRect: in_view toView: nil];
            msg_send![window.native_window, convertRectToScreen: in_window]
        }
    }

    fn hit_test(&self, point: Point<Pixels>) -> Option<AccessibilityNodeId> {
        self.nodes
            .iter()
            .rev()
            .find(|n| n.bounds.contains(&point))
            .map(|n| n.id)
    }

    fn focused(&self) -> Option<AccessibilityNodeId> {
        self.nodes.iter().find(|n| n.focused).map(|n| n.id)
    }
}

fn retain(object: id) -> id {
    if object != nil {
        unsafe {
            let _: id = msg_send![object, retain];
        }
    }
    object
}

pub(super) fn post_notifications(notes: Vec<(id, &'static str)>) {
    for (element, name) in notes {
        if element == nil {
            continue;
        }
        unsafe {
            NSAccessibilityPostNotification(element, ns_string(name));
            let _: () = msg_send![element, release];
        }
    }
}

impl MacWindow {
    pub(super) fn set_accessibility_request_callback(
        &self,
        callback: Box<dyn FnMut(AccessibilityRequest)>,
    ) {
        self.0.lock().accessibility.request_callback = Some(callback);
    }

    pub(super) fn update_accessibility_nodes(&self, nodes: &[AccessibilityNode]) {
        let weak = Arc::downgrade(&self.0);
        let notes = {
            let mut lock = self.0.lock();
            let native_window = lock.native_window;
            lock.accessibility.update(nodes, &weak, native_window)
        };
        post_notifications(notes);
    }
}

fn send_request(state: &Arc<Mutex<MacWindowState>>, request: AccessibilityRequest) -> bool {
    let Some(mut callback) = state
        .try_lock()
        .and_then(|mut lock| lock.accessibility.request_callback.take())
    else {
        return false;
    };
    callback(request);
    if let Some(mut lock) = state.try_lock() {
        lock.accessibility.request_callback.get_or_insert(callback);
    }
    true
}

/// Starts collecting nodes the first time anything asks for them.
fn activate(state: &Arc<Mutex<MacWindowState>>) {
    let needs_activation = state.try_lock().is_some_and(|mut lock| {
        let first = !lock.accessibility.active;
        lock.accessibility.active = true;
        first
    });
    if needs_activation && !send_request(state, AccessibilityRequest::Activate) {
        if let Some(mut lock) = state.try_lock() {
            lock.accessibility.active = false;
        }
    }
}

pub(super) unsafe fn add_view_methods(decl: &mut ClassDecl) {
    unsafe {
        decl.add_method(
            sel!(isAccessibilityElement),
            yes as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(accessibilityRole),
            view_accessibility_role as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityChildren),
            view_accessibility_children as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityHitTest:),
            view_accessibility_hit_test as extern "C" fn(&Object, Sel, NSPoint) -> id,
        );
        decl.add_method(
            sel!(accessibilityFocusedUIElement),
            view_accessibility_focused_element as extern "C" fn(&Object, Sel) -> id,
        );
    }
}

extern "C" fn view_accessibility_role(_: &Object, _: Sel) -> id {
    unsafe { ns_string("AXGroup") }
}

extern "C" fn view_accessibility_children(this: &Object, _: Sel) -> id {
    let state = unsafe { get_window_state(this) };
    activate(&state);
    match state.try_lock() {
        Some(lock) => lock.accessibility.array(&lock.accessibility.roots),
        None => unsafe { NSArray::arrayWithObjects(nil, &[]) },
    }
}

extern "C" fn view_accessibility_hit_test(this: &Object, _: Sel, point: NSPoint) -> id {
    let state = unsafe { get_window_state(this) };
    activate(&state);
    let hit = state.try_lock().and_then(|lock| unsafe {
        let view = lock.native_view.as_ptr() as id;
        let in_window: NSPoint = msg_send![lock.native_window, convertPointFromScreen: point];
        let in_view: NSPoint = msg_send![view, convertPoint: in_window fromView: nil];
        let height = NSView::frame(view).size.height;
        let point = Point::new(px(in_view.x as f32), px((height - in_view.y) as f32));
        let id = lock.accessibility.hit_test(point)?;
        Some(lock.accessibility.element(id))
    });
    match hit {
        Some(element) if element != nil => element,
        _ => unsafe { msg_send![super(this, class!(NSView)), accessibilityHitTest: point] },
    }
}

extern "C" fn view_accessibility_focused_element(this: &Object, _: Sel) -> id {
    let state = unsafe { get_window_state(this) };
    activate(&state);
    let focused = state.try_lock().and_then(|lock| {
        let id = lock.accessibility.focused()?;
        Some(lock.accessibility.element(id))
    });
    match focused {
        Some(element) if element != nil => element,
        _ => unsafe { msg_send![super(this, class!(NSView)), accessibilityFocusedUIElement] },
    }
}

pub(super) unsafe fn build_element_class() {
    unsafe {
        let mut decl =
            ClassDecl::new("GPUIAccessibilityElement", class!(NSAccessibilityElement)).unwrap();
        decl.add_ivar::<*mut c_void>(NODE_IVAR);
        decl.add_method(sel!(dealloc), element_dealloc as extern "C" fn(&Object, Sel));
        decl.add_method(
            sel!(isAccessibilityElement),
            yes as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(isAccessibilityEnabled),
            yes as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(accessibilityRole),
            element_role as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilitySubrole),
            element_subrole as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityRoleDescription),
            element_role_description as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityLabel),
            element_label as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityValue),
            element_value as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(isAccessibilitySelected),
            element_selected as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(isAccessibilityFocused),
            element_focused as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(setAccessibilityFocused:),
            element_set_focused as extern "C" fn(&Object, Sel, BOOL),
        );
        decl.add_method(
            sel!(accessibilityFrame),
            element_frame as extern "C" fn(&Object, Sel) -> NSRect,
        );
        decl.add_method(
            sel!(accessibilityParent),
            element_parent as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityChildren),
            element_children as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityWindow),
            element_window as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityTopLevelUIElement),
            element_window as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(accessibilityPerformPress),
            element_press as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(isAccessibilitySelectorAllowed:),
            element_selector_allowed as extern "C" fn(&Object, Sel, Sel) -> BOOL,
        );
        ELEMENT_CLASS = decl.register();
    }
}

fn new_element(state: Weak<Mutex<MacWindowState>>, id: AccessibilityNodeId) -> id {
    unsafe {
        let element: id = msg_send![ELEMENT_CLASS, alloc];
        let element: id = msg_send![element, init];
        let data = Box::into_raw(Box::new(ElementData { state, id }));
        (*element).set_ivar(NODE_IVAR, data as *mut c_void);
        element
    }
}

extern "C" fn element_dealloc(this: &Object, _: Sel) {
    unsafe {
        let data: *mut c_void = *this.get_ivar(NODE_IVAR);
        if !data.is_null() {
            drop(Box::from_raw(data as *mut ElementData));
        }
        let _: () = msg_send![super(this, class!(NSAccessibilityElement)), dealloc];
    }
}

fn with_node<R>(
    this: &Object,
    f: impl FnOnce(&MacWindowState, &AccessibilityNode) -> R,
) -> Option<R> {
    let data = unsafe {
        let data: *mut c_void = *this.get_ivar(NODE_IVAR);
        (data as *const ElementData).as_ref()?
    };
    let state = data.state.upgrade()?;
    let lock = state.try_lock()?;
    let node = lock.accessibility.node(data.id)?;
    Some(f(&lock, node))
}

fn element_state(this: &Object) -> Option<(Arc<Mutex<MacWindowState>>, AccessibilityNodeId)> {
    unsafe {
        let data: *mut c_void = *this.get_ivar(NODE_IVAR);
        let data = (data as *const ElementData).as_ref()?;
        Some((data.state.upgrade()?, data.id))
    }
}

fn role_names(role: Role) -> (&'static str, Option<&'static str>) {
    match role {
        Role::Group => ("AXGroup", None),
        Role::Button => ("AXButton", None),
        Role::ToggleButton => ("AXCheckBox", Some("AXToggle")),
        Role::CheckBox => ("AXCheckBox", None),
        Role::RadioButton => ("AXRadioButton", None),
        Role::Tab => ("AXRadioButton", Some("AXTabButton")),
        Role::Link => ("AXLink", None),
        Role::StaticText | Role::Heading => ("AXStaticText", None),
        Role::TextField => ("AXTextField", None),
        Role::SearchField => ("AXTextField", Some("AXSearchField")),
        Role::Slider => ("AXSlider", None),
        Role::Image => ("AXImage", None),
        Role::List => ("AXList", None),
        Role::Row => ("AXRow", None),
    }
}

fn ns_string_or_nil(s: Option<&str>) -> id {
    match s {
        Some(s) if !s.is_empty() => unsafe { ns_string(s) },
        _ => nil,
    }
}

extern "C" fn element_role(this: &Object, _: Sel) -> id {
    let role = with_node(this, |_, n| role_names(n.role).0).unwrap_or("AXUnknown");
    unsafe { ns_string(role) }
}

extern "C" fn element_subrole(this: &Object, _: Sel) -> id {
    ns_string_or_nil(with_node(this, |_, n| role_names(n.role).1).flatten())
}

extern "C" fn element_role_description(this: &Object, _: Sel) -> id {
    let Some(role) = with_node(this, |_, n| n.role) else {
        return nil;
    };
    match role {
        Role::Heading => unsafe { ns_string("heading") },
        Role::Row => unsafe { ns_string("row") },
        _ => {
            let (role, subrole) = role_names(role);
            unsafe { NSAccessibilityRoleDescription(ns_string(role), ns_string_or_nil(subrole)) }
        }
    }
}

extern "C" fn element_label(this: &Object, _: Sel) -> id {
    let label = with_node(this, |_, n| {
        (!matches!(n.role, Role::StaticText | Role::Heading)).then(|| n.label.clone())
    });
    ns_string_or_nil(label.flatten().as_ref().map(|l| l.as_ref()))
}

extern "C" fn element_value(this: &Object, _: Sel) -> id {
    enum Value {
        Number(bool),
        Text(SharedString),
        None,
    }
    let value = with_node(this, |_, n| {
        if let Some(on) = n.toggled {
            Value::Number(on)
        } else if matches!(n.role, Role::RadioButton | Role::Tab) {
            Value::Number(n.selected == Some(true))
        } else if let Some(value) = &n.value {
            Value::Text(value.clone())
        } else if matches!(n.role, Role::StaticText | Role::Heading) {
            Value::Text(n.label.clone())
        } else {
            Value::None
        }
    });
    match value {
        Some(Value::Number(on)) => unsafe {
            msg_send![class!(NSNumber), numberWithInteger: on as NSInteger]
        },
        Some(Value::Text(text)) => unsafe { ns_string(&text) },
        _ => nil,
    }
}

extern "C" fn element_selected(this: &Object, _: Sel) -> BOOL {
    with_node(this, |_, n| n.selected == Some(true))
        .unwrap_or(false)
        .to_objc()
}

extern "C" fn element_focused(this: &Object, _: Sel) -> BOOL {
    with_node(this, |_, n| n.focused).unwrap_or(false).to_objc()
}

extern "C" fn element_set_focused(this: &Object, _: Sel, focused: BOOL) {
    if focused == YES
        && let Some((state, id)) = element_state(this)
    {
        send_request(&state, AccessibilityRequest::Focus(id));
    }
}

extern "C" fn element_frame(this: &Object, _: Sel) -> NSRect {
    with_node(this, |window, n| window.accessibility.screen_frame(n, window))
        .unwrap_or_else(|| NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)))
}

extern "C" fn element_parent(this: &Object, _: Sel) -> id {
    with_node(this, |window, n| match n.parent {
        Some(parent) if window.accessibility.node(parent).is_some() => {
            window.accessibility.element(parent)
        }
        _ => window.native_view.as_ptr() as id,
    })
    .unwrap_or(nil)
}

extern "C" fn element_children(this: &Object, _: Sel) -> id {
    with_node(this, |window, n| {
        let children = window
            .accessibility
            .children
            .get(&n.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        window.accessibility.array(children)
    })
    .unwrap_or_else(|| unsafe { NSArray::arrayWithObjects(nil, &[]) })
}

extern "C" fn element_window(this: &Object, _: Sel) -> id {
    with_node(this, |window, _| window.native_window).unwrap_or(nil)
}

extern "C" fn element_press(this: &Object, _: Sel) -> BOOL {
    let Some((state, id)) = element_state(this) else {
        return NO;
    };
    let pressable = state
        .try_lock()
        .and_then(|lock| lock.accessibility.node(id).map(|n| n.pressable))
        .unwrap_or(false);
    (pressable && send_request(&state, AccessibilityRequest::Press(id))).to_objc()
}

extern "C" fn element_selector_allowed(this: &Object, _: Sel, selector: Sel) -> BOOL {
    if selector == sel!(accessibilityPerformPress) {
        return with_node(this, |_, n| n.pressable).unwrap_or(false).to_objc();
    }
    if selector == sel!(setAccessibilityFocused:) {
        return with_node(this, |_, n| n.focusable).unwrap_or(false).to_objc();
    }
    unsafe {
        msg_send![super(this, class!(NSAccessibilityElement)), isAccessibilitySelectorAllowed: selector]
    }
}
