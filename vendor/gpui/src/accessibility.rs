//! Describing elements to assistive technologies such as VoiceOver.
//!
//! Elements opt in with [`InteractiveElement::accessibility_label`] and friends. Elements with
//! `on_click` are exposed as buttons without any of that, labeled by the text inside them.
//! Nothing is collected until the platform reports that an assistive technology is reading
//! the window.

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::{
    App, Bounds, ClickEvent, ClickListener, FocusHandle, GlobalElementId, KeyboardButton,
    KeyboardClickEvent, Pixels, SharedString, Window,
};

/// What kind of thing an element is, for assistive technologies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
    /// A container that groups other elements.
    #[default]
    Group,
    /// Something that does an action when pressed.
    Button,
    /// A button that stays on or off; give it [`InteractiveElement::accessibility_toggled`].
    ToggleButton,
    /// A check box; give it [`InteractiveElement::accessibility_toggled`].
    CheckBox,
    /// One of a set of mutually exclusive options; mark the chosen one selected.
    RadioButton,
    /// A tab in a tab bar; mark the current one selected.
    Tab,
    /// A link to another page.
    Link,
    /// Text that can't be edited.
    StaticText,
    /// A heading over a section.
    Heading,
    /// An editable text field; its value is the text.
    TextField,
    /// A search field; its value is the query.
    SearchField,
    /// A slider; its value is the position.
    Slider,
    /// An image.
    Image,
    /// A list of rows.
    List,
    /// A row in a list or table.
    Row,
}

impl Role {
    /// Whether it's read as one thing: its label covers the text inside it, and is that
    /// text when no label is given.
    pub fn reads_as_one(self) -> bool {
        !matches!(self, Role::Group | Role::StaticText | Role::Image | Role::List)
    }
}

/// A stable identifier for an accessible element, the same from frame to frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccessibilityNodeId(pub u64);

/// An element of the current frame, as handed to the platform.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityNode {
    /// Its stable identifier.
    pub id: AccessibilityNodeId,
    /// The accessible element it's inside, if any.
    pub parent: Option<AccessibilityNodeId>,
    /// What it is.
    pub role: Role,
    /// What it's called.
    pub label: SharedString,
    /// Its current value, such as a slider position or a text field's text.
    pub value: Option<SharedString>,
    /// Whether a toggle or check box is on.
    pub toggled: Option<bool>,
    /// Whether it's the selected one of its kind, such as the current tab.
    pub selected: Option<bool>,
    /// Whether it has keyboard focus.
    pub focused: bool,
    /// Whether it can take keyboard focus.
    pub focusable: bool,
    /// Whether pressing it does something.
    pub pressable: bool,
    /// Where it is, in window coordinates, clipped to what's visible.
    pub bounds: Bounds<Pixels>,
}

/// What the platform asks of a window on behalf of an assistive technology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityRequest {
    /// Something started reading the window; start collecting its elements.
    Activate,
    /// Press the element, like a click.
    Press(AccessibilityNodeId),
    /// Move keyboard focus to the element.
    Focus(AccessibilityNodeId),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AccessibilityProps {
    pub(crate) role: Option<Role>,
    pub(crate) label: Option<SharedString>,
    pub(crate) value: Option<SharedString>,
    pub(crate) toggled: Option<bool>,
    pub(crate) selected: Option<bool>,
    pub(crate) hidden: bool,
}

pub(crate) struct AccessibilityEntry {
    pub(crate) node: AccessibilityNode,
    pub(crate) click_listeners: Vec<ClickListener>,
    pub(crate) focus_handle: Option<FocusHandle>,
}

impl Clone for AccessibilityEntry {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
            click_listeners: self.click_listeners.clone(),
            focus_handle: self.focus_handle.clone(),
        }
    }
}

struct StackEntry {
    entry_ix: Option<usize>,
    id: u64,
    children: u32,
    hidden: bool,
    /// Text inside goes into this node's label rather than becoming nodes of its own.
    collect_text: Option<String>,
    /// The label already says what text inside would.
    labeled: bool,
}

#[derive(Default)]
pub(crate) struct AccessibilityBuilder {
    stack: Vec<StackEntry>,
    root_children: u32,
    pub(crate) ids: collections::FxHashSet<u64>,
}

const MAX_COLLECTED_LABEL: usize = 300;

fn hash_one(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl Window {
    /// Whether an assistive technology is reading this window, so elements are being collected.
    pub fn accessibility_enabled(&self) -> bool {
        self.accessibility_enabled
    }

    fn next_child_id(&mut self, salt: u64) -> u64 {
        let builder = &mut self.accessibility_builder;
        match builder.stack.last_mut() {
            Some(parent) => {
                parent.children += 1;
                hash_one((parent.id, parent.children, salt))
            }
            None => {
                builder.root_children += 1;
                hash_one((0u64, builder.root_children, salt))
            }
        }
    }

    fn accessible_parent(&self) -> Option<AccessibilityNodeId> {
        self.accessibility_builder
            .stack
            .iter()
            .rev()
            .find_map(|e| e.entry_ix.map(|_| AccessibilityNodeId(e.id)))
    }

    /// Starts an element during paint. Returns whether it was pushed, in which case
    /// [`Window::end_accessibility_node`] must follow once its children have painted.
    pub(crate) fn begin_accessibility_node(
        &mut self,
        global_id: Option<&GlobalElementId>,
        props: Option<&AccessibilityProps>,
        bounds: Bounds<Pixels>,
        click_listeners: &[ClickListener],
        focus_handle: Option<&FocusHandle>,
        opacity: f32,
    ) -> bool {
        if self
            .accessibility_builder
            .stack
            .last()
            .is_some_and(|e| e.hidden)
        {
            return false;
        }
        if props.is_some_and(|p| p.hidden) || opacity <= 0.01 {
            self.accessibility_builder.stack.push(StackEntry {
                entry_ix: None,
                id: 0,
                children: 0,
                hidden: true,
                collect_text: None,
                labeled: false,
            });
            return true;
        }
        let explicit_role = props.and_then(|p| p.role);
        let pressable = !click_listeners.is_empty();
        let wants_node = explicit_role.is_some()
            || props.is_some_and(|p| p.label.is_some())
            || pressable;
        if !wants_node {
            return false;
        }

        let visible = bounds.intersect(&self.content_mask().bounds);
        if visible.size.width <= Pixels::ZERO || visible.size.height <= Pixels::ZERO {
            return false;
        }

        let props = props.cloned().unwrap_or_default();
        let role = explicit_role.unwrap_or(if pressable { Role::Button } else { Role::Group });
        let focuses_on_press = matches!(role, Role::TextField | Role::SearchField);
        let parent = self.accessible_parent();
        let positional = self.next_child_id(role as u64);
        let mut id = match global_id {
            Some(global_id) => hash_one(global_id),
            None => positional,
        };
        while !self.accessibility_builder.ids.insert(id) {
            id = hash_one((id, positional));
        }
        let collect_text = (role.reads_as_one() && props.label.is_none()).then(String::new);
        let labeled = props.label.is_some();

        let entry_ix = self.next_frame.accessibility.len();
        self.next_frame.accessibility.push(AccessibilityEntry {
            node: AccessibilityNode {
                id: AccessibilityNodeId(id),
                parent,
                role,
                label: props.label.unwrap_or_default(),
                value: props.value,
                toggled: props.toggled,
                selected: props.selected,
                focused: focus_handle.is_some_and(|h| h.is_focused(self)),
                focusable: focus_handle.is_some(),
                pressable: pressable || (focuses_on_press && focus_handle.is_some()),
                bounds: visible,
            },
            click_listeners: click_listeners.to_vec(),
            focus_handle: focus_handle.cloned(),
        });
        self.accessibility_builder.stack.push(StackEntry {
            entry_ix: Some(entry_ix),
            id,
            children: 0,
            hidden: false,
            collect_text,
            labeled,
        });
        true
    }

    pub(crate) fn end_accessibility_node(&mut self) {
        let Some(entry) = self.accessibility_builder.stack.pop() else {
            return;
        };
        if let (Some(ix), Some(text)) = (entry.entry_ix, entry.collect_text)
            && !text.is_empty()
        {
            self.next_frame.accessibility[ix].node.label = text.into();
        }
    }

    /// Records text drawn during paint: it labels the control it's inside, or becomes
    /// static text of its own.
    pub(crate) fn accessibility_text(&mut self, text: &str, bounds: Bounds<Pixels>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let builder = &mut self.accessibility_builder;
        if builder.stack.last().is_some_and(|e| e.hidden) {
            return;
        }
        for entry in builder.stack.iter_mut().rev() {
            if let Some(collector) = entry.collect_text.as_mut() {
                if collector.len() < MAX_COLLECTED_LABEL {
                    if !collector.is_empty() {
                        collector.push_str(", ");
                    }
                    collector.push_str(text);
                }
                return;
            }
            if entry.labeled {
                return;
            }
        }
        let visible = bounds.intersect(&self.content_mask().bounds);
        if visible.size.width <= Pixels::ZERO || visible.size.height <= Pixels::ZERO {
            return;
        }
        let parent = self.accessible_parent();
        let mut id = self.next_child_id(Role::StaticText as u64);
        while !self.accessibility_builder.ids.insert(id) {
            id = hash_one(id);
        }
        self.next_frame.accessibility.push(AccessibilityEntry {
            node: AccessibilityNode {
                id: AccessibilityNodeId(id),
                parent,
                role: Role::StaticText,
                label: SharedString::from(text.to_string()),
                value: None,
                toggled: None,
                selected: None,
                focused: false,
                focusable: false,
                pressable: false,
                bounds: visible,
            },
            click_listeners: Vec::new(),
            focus_handle: None,
        });
    }

    /// Hands the frame's elements to the platform when they differ from what it has.
    pub(crate) fn sync_accessibility(&mut self) {
        self.accessibility_builder.stack.clear();
        self.accessibility_builder.root_children = 0;
        self.accessibility_builder.ids.clear();
        if !self.accessibility_enabled {
            return;
        }
        let entries = &mut self.rendered_frame.accessibility;
        let mut seen = collections::FxHashSet::default();
        for (ix, entry) in entries.iter_mut().enumerate() {
            while !seen.insert(entry.node.id) {
                entry.node.id = AccessibilityNodeId(hash_one((entry.node.id.0, ix)));
            }
        }
        let unchanged = entries.len() == self.accessibility_sent.len()
            && entries
                .iter()
                .zip(&self.accessibility_sent)
                .all(|(e, sent)| e.node == *sent);
        if unchanged {
            return;
        }
        self.accessibility_sent = entries.iter().map(|e| e.node.clone()).collect();
        self.platform_window
            .update_accessibility(&self.accessibility_sent);
    }

    pub(crate) fn handle_accessibility_request(
        &mut self,
        request: AccessibilityRequest,
        cx: &mut App,
    ) {
        match request {
            AccessibilityRequest::Activate => {
                if !self.accessibility_enabled {
                    self.accessibility_enabled = true;
                    self.refresh();
                }
            }
            AccessibilityRequest::Press(id) => {
                let Some(entry) = self
                    .rendered_frame
                    .accessibility
                    .iter()
                    .find(|e| e.node.id == id)
                    .cloned()
                else {
                    return;
                };
                if entry.click_listeners.is_empty() {
                    if let Some(handle) = &entry.focus_handle {
                        self.focus(handle);
                    }
                } else {
                    let event = ClickEvent::Keyboard(KeyboardClickEvent {
                        button: KeyboardButton::Enter,
                        bounds: entry.node.bounds,
                    });
                    for listener in &entry.click_listeners {
                        listener(&event, self, cx);
                    }
                }
                self.refresh();
            }
            AccessibilityRequest::Focus(id) => {
                if let Some(handle) = self
                    .rendered_frame
                    .accessibility
                    .iter()
                    .find(|e| e.node.id == id)
                    .and_then(|e| e.focus_handle.clone())
                {
                    self.focus(&handle);
                }
            }
        }
    }
}
