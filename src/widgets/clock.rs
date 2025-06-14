use gtk4::prelude::*;
use gtk4::{Label, Button, Popover, Box, Orientation, Calendar, ScrolledWindow, Entry, ListBox, ListBoxRow, CheckButton, ApplicationWindow, Separator};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use glib::timeout_add_seconds_local;
use chrono::{Local, Datelike, NaiveDate};
use anyhow::Result;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    id: String,
    text: String,
    completed: bool,
    created_date: String,
    due_date: Option<String>, // None means it's a general todo
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TodoStore {
    todos: HashMap<String, TodoItem>, // id -> TodoItem
    next_id: u64,
}

impl TodoStore {
    fn new() -> Self {
        Self {
            todos: HashMap::new(),
            next_id: 1,
        }
    }
    
    fn add_todo(&mut self, text: String, due_date: Option<String>) -> String {
        let id = format!("todo_{}", self.next_id);
        self.next_id += 1;
        
        let todo = TodoItem {
            id: id.clone(),
            text,
            completed: false,
            created_date: Local::now().format("%Y-%m-%d").to_string(),
            due_date,
        };
        
        self.todos.insert(id.clone(), todo);
        id
    }
    
    fn toggle_todo(&mut self, id: &str) -> Option<bool> {
        self.todos.get_mut(id).map(|todo| {
            todo.completed = !todo.completed;
            todo.completed
        })
    }
    
    fn delete_todo(&mut self, id: &str) -> bool {
        self.todos.remove(id).is_some()
    }
    
    fn get_todos_for_date(&self, date: &str) -> Vec<&TodoItem> {
        let mut todos = Vec::new();
        let today = Local::now().format("%Y-%m-%d").to_string();
        
        // Parse the requested date
        let requested_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
        let today_date = NaiveDate::parse_from_str(&today, "%Y-%m-%d").ok();
        
        for todo in self.todos.values() {
            // Skip completed todos
            if todo.completed {
                continue;
            }
            
            if let Some(due_date) = &todo.due_date {
                // Todo has a specific due date
                if due_date == date {
                    // Show on the exact due date
                    todos.push(todo);
                } else if date == &today {
                    // On today's view, also show overdue todos
                    if let (Some(due), Some(today), Some(requested)) = (
                        NaiveDate::parse_from_str(due_date, "%Y-%m-%d").ok(),
                        today_date,
                        requested_date
                    ) {
                        if due < today && requested == today {
                            todos.push(todo);
                        }
                    }
                }
            } else if date == &today {
                // General todos (no due date) only show on today
                todos.push(todo);
            }
        }
        
        // Sort by due date (overdue first, then no due date)
        todos.sort_by(|a, b| {
            match (&a.due_date, &b.due_date) {
                (Some(a_date), Some(b_date)) => a_date.cmp(b_date),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        
        todos
    }
    
    fn get_completed_todos_for_date(&self, date: &str) -> Vec<&TodoItem> {
        let mut todos = Vec::new();
        
        for todo in self.todos.values() {
            if !todo.completed {
                continue;
            }
            
            // Show completed todos on their due date or creation date
            if let Some(due_date) = &todo.due_date {
                if due_date == date {
                    todos.push(todo);
                }
            } else if &todo.created_date == date {
                todos.push(todo);
            }
        }
        
        todos
    }
}

pub struct Clock {
    button: Button,
}

impl Clock {
    pub fn new(
        format: &str,
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("clock");
        
        let label = Label::new(None);
        button.set_child(Some(&label));
        
        // Create popover for calendar
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("calendar-popover");
        popover.set_has_arrow(false);
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Clock popover shown - keyboard mode set to OnDemand (active popovers: {})", 
                      *active_popovers_show.borrow());
            }
        });
        
        // Handle popover hide event - disable keyboard mode if no other popovers
        let window_weak_hide = window_weak.clone();
        let active_popovers_hide = active_popovers.clone();
        popover.connect_hide(move |_| {
            *active_popovers_hide.borrow_mut() -= 1;
            let count = *active_popovers_hide.borrow();
            if count == 0 {
                if let Some(window) = window_weak_hide.upgrade() {
                    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
                    info!("Clock popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Clock popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        let main_box = Box::new(Orientation::Vertical, 10);
        main_box.set_margin_top(15);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);
        main_box.set_size_request(450, -1);
        
        // Calendar widget
        let calendar = Calendar::new();
        calendar.add_css_class("clock-calendar");
        main_box.append(&calendar);
        
        // Date label
        let date_label = Label::new(None);
        date_label.set_halign(gtk4::Align::Center);
        date_label.add_css_class("calendar-date-label");
        main_box.append(&date_label);
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        main_box.append(&separator);
        
        // Todo section header
        let todo_header = Box::new(Orientation::Horizontal, 5);
        
        let todo_label = Label::new(Some("Todos"));
        todo_label.set_halign(gtk4::Align::Start);
        todo_label.add_css_class("calendar-section-label");
        todo_label.set_hexpand(true);
        todo_header.append(&todo_label);
        
        let todo_count_label = Label::new(Some("0"));
        todo_count_label.add_css_class("calendar-todo-count");
        todo_header.append(&todo_count_label);
        
        main_box.append(&todo_header);
        
        // Todo input
        let todo_box = Box::new(Orientation::Horizontal, 5);
        let todo_entry = Entry::new();
        todo_entry.set_placeholder_text(Some("Add a todo..."));
        todo_entry.set_hexpand(true);
        todo_entry.add_css_class("calendar-todo-entry");
        
        let add_button = Button::with_label("Add");
        add_button.add_css_class("calendar-todo-add");
        
        todo_box.append(&todo_entry);
        todo_box.append(&add_button);
        main_box.append(&todo_box);
        
        // Active todos list
        let active_scroll = ScrolledWindow::new();
        active_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        active_scroll.set_min_content_height(100);
        active_scroll.set_max_content_height(250);
        
        let active_todo_list = ListBox::new();
        active_todo_list.add_css_class("calendar-todo-list");
        active_todo_list.set_selection_mode(gtk4::SelectionMode::None);
        
        active_scroll.set_child(Some(&active_todo_list));
        main_box.append(&active_scroll);
        
        // Completed todos section
        let completed_separator = gtk4::Separator::new(Orientation::Horizontal);
        completed_separator.set_margin_top(10);
        completed_separator.set_margin_bottom(5);
        main_box.append(&completed_separator);
        
        let completed_header = Box::new(Orientation::Horizontal, 5);
        
        let completed_label = Label::new(Some("Completed"));
        completed_label.set_halign(gtk4::Align::Start);
        completed_label.add_css_class("calendar-section-label");
        completed_label.set_hexpand(true);
        completed_header.append(&completed_label);
        
        let completed_count_label = Label::new(Some("0"));
        completed_count_label.add_css_class("calendar-todo-count");
        completed_header.append(&completed_count_label);
        
        main_box.append(&completed_header);
        
        // Completed todos list
        let completed_scroll = ScrolledWindow::new();
        completed_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        completed_scroll.set_min_content_height(50);
        completed_scroll.set_max_content_height(150);
        
        let completed_todo_list = ListBox::new();
        completed_todo_list.add_css_class("calendar-todo-list");
        completed_todo_list.set_selection_mode(gtk4::SelectionMode::None);
        
        completed_scroll.set_child(Some(&completed_todo_list));
        main_box.append(&completed_scroll);
        
        popover.set_child(Some(&main_box));
        
        // Load todo store
        let todo_store = Rc::new(RefCell::new(Self::load_todo_store()));
        
        // Update time immediately
        let format_clone = format.to_string();
        Self::update_time(&label, &format_clone);
        
        // Update every second
        let label_weak = label.downgrade();
        let format_for_timer = format.to_string();
        timeout_add_seconds_local(1, move || {
            if let Some(label) = label_weak.upgrade() {
                Self::update_time(&label, &format_for_timer);
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        // Handle calendar selection
        let todo_store_clone = todo_store.clone();
        let active_list_weak = active_todo_list.downgrade();
        let completed_list_weak = completed_todo_list.downgrade();
        let date_label_weak = date_label.downgrade();
        let todo_count_weak = todo_count_label.downgrade();
        let completed_count_weak = completed_count_label.downgrade();
        calendar.connect_day_selected(move |cal| {
            let date = format!("{}-{:02}-{:02}", 
                cal.year(), cal.month() + 1, cal.day());
            
            if let (Some(active_list), Some(completed_list), Some(date_lbl), 
                    Some(todo_count), Some(completed_count)) = 
                (active_list_weak.upgrade(), completed_list_weak.upgrade(), 
                 date_label_weak.upgrade(), todo_count_weak.upgrade(), 
                 completed_count_weak.upgrade()) {
                
                // Update date label
                let today = Local::now().format("%Y-%m-%d").to_string();
                if date == today {
                    date_lbl.set_text("Today");
                } else {
                    let parsed_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                        .unwrap_or_else(|_| Local::now().date_naive());
                    date_lbl.set_text(&parsed_date.format("%A, %B %d, %Y").to_string());
                }
                
                // Update todo lists
                Self::update_todo_lists(&active_list, &completed_list, &date, 
                                      &todo_store_clone, &todo_count, &completed_count);
            }
        });
        
        // Handle todo addition
        let todo_store_for_add = todo_store.clone();
        let calendar_for_add = calendar.downgrade();
        let active_list_for_add = active_todo_list.downgrade();
        let completed_list_for_add = completed_todo_list.downgrade();
        let todo_entry_weak = todo_entry.downgrade();
        let todo_count_for_add = todo_count_label.downgrade();
        let completed_count_for_add = completed_count_label.downgrade();
        
        let add_todo = move || {
            if let (Some(cal), Some(active_list), Some(completed_list), Some(entry),
                    Some(todo_count), Some(completed_count)) = 
                (calendar_for_add.upgrade(), active_list_for_add.upgrade(), 
                 completed_list_for_add.upgrade(), todo_entry_weak.upgrade(),
                 todo_count_for_add.upgrade(), completed_count_for_add.upgrade()) {
                
                let text = entry.text();
                if !text.is_empty() {
                    let date = format!("{}-{:02}-{:02}", 
                        cal.year(), cal.month() + 1, cal.day());
                    
                    let today = Local::now().format("%Y-%m-%d").to_string();
                    let due_date = if date == today {
                        None // General todo
                    } else {
                        Some(date.clone()) // Specific due date
                    };
                    
                    let mut store = todo_store_for_add.borrow_mut();
                    store.add_todo(text.to_string(), due_date);
                    Self::save_todo_store(&store);
                    drop(store);
                    
                    // Refresh the lists
                    Self::update_todo_lists(&active_list, &completed_list, &date, 
                                          &todo_store_for_add, &todo_count, &completed_count);
                    
                    entry.set_text("");
                }
            }
        };
        
        // Connect add button
        let add_todo_click = add_todo.clone();
        add_button.connect_clicked(move |_| {
            add_todo_click();
        });
        
        // Handle Enter key in todo entry
        todo_entry.connect_activate(move |_| {
            add_todo();
        });
        
        // Handle Escape key to close popover
        let escape_controller = gtk4::EventControllerKey::new();
        let popover_weak_escape = popover.downgrade();
        escape_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(popover) = popover_weak_escape.upgrade() {
                    popover.popdown();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        todo_entry.add_controller(escape_controller);
        
        // Show popover on click
        let calendar_for_show = calendar.downgrade();
        let active_list_for_show = active_todo_list.downgrade();
        let completed_list_for_show = completed_todo_list.downgrade();
        let date_label_for_show = date_label.downgrade();
        let todo_count_for_show = todo_count_label.downgrade();
        let completed_count_for_show = completed_count_label.downgrade();
        let todo_store_for_show = todo_store.clone();
        
        button.connect_clicked(move |_| {
            // Set calendar to current date and trigger update
            if let (Some(cal), Some(active), Some(completed), Some(date_lbl), 
                    Some(t_count), Some(c_count)) = 
                (calendar_for_show.upgrade(), active_list_for_show.upgrade(),
                 completed_list_for_show.upgrade(), date_label_for_show.upgrade(),
                 todo_count_for_show.upgrade(), completed_count_for_show.upgrade()) {
                
                let now = Local::now();
                let datetime = gtk4::glib::DateTime::from_local(
                    now.year(),
                    now.month() as i32,
                    now.day() as i32,
                    0, 0, 0.0
                ).unwrap();
                cal.select_day(&datetime);
                
                // Manually trigger initial update
                let today = now.format("%Y-%m-%d").to_string();
                date_lbl.set_text("Today");
                Self::update_todo_lists(&active, &completed, &today, 
                                      &todo_store_for_show, &t_count, &c_count);
            }
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn update_todo_lists(active_list: &ListBox, completed_list: &ListBox, date: &str, 
                        todo_store: &Rc<RefCell<TodoStore>>, 
                        todo_count: &Label, completed_count: &Label) {
        // Clear existing items
        while let Some(child) = active_list.first_child() {
            active_list.remove(&child);
        }
        while let Some(child) = completed_list.first_child() {
            completed_list.remove(&child);
        }
        
        let store = todo_store.borrow();
        let active_todos = store.get_todos_for_date(date);
        let completed_todos = store.get_completed_todos_for_date(date);
        
        // Update counts
        todo_count.set_text(&active_todos.len().to_string());
        completed_count.set_text(&completed_todos.len().to_string());
        
        // Add active todos
        if active_todos.is_empty() {
            let row = ListBoxRow::new();
            let label = Label::new(Some("No todos"));
            label.add_css_class("dim-label");
            label.set_margin_top(20);
            label.set_margin_bottom(20);
            row.set_child(Some(&label));
            active_list.append(&row);
        } else {
            for todo in active_todos {
                Self::add_todo_row(active_list, todo, date, todo_store.clone(), false);
            }
        }
        
        // Add completed todos
        if !completed_todos.is_empty() {
            for todo in completed_todos {
                Self::add_todo_row(completed_list, todo, date, todo_store.clone(), true);
            }
        }
    }
    
    fn add_todo_row(list: &ListBox, todo: &TodoItem, current_date: &str, 
                    todo_store: Rc<RefCell<TodoStore>>, in_completed_section: bool) {
        let row = ListBoxRow::new();
        row.add_css_class("calendar-todo-item");
        
        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(5);
        hbox.set_margin_end(5);
        hbox.set_margin_top(5);
        hbox.set_margin_bottom(5);
        
        let check = CheckButton::new();
        check.set_active(todo.completed);
        hbox.append(&check);
        
        let vbox = Box::new(Orientation::Vertical, 2);
        vbox.set_hexpand(true);
        
        let label = Label::new(Some(&todo.text));
        label.set_halign(gtk4::Align::Start);
        if todo.completed {
            label.add_css_class("calendar-todo-completed");
        }
        vbox.append(&label);
        
        // Show due date if different from current view
        let today = Local::now().format("%Y-%m-%d").to_string();
        if let Some(due_date) = &todo.due_date {
            if due_date != current_date && !in_completed_section {
                let due_label = Label::new(None);
                due_label.set_halign(gtk4::Align::Start);
                due_label.add_css_class("calendar-todo-due");
                
                // Check if overdue
                if let (Ok(due), Ok(today_date)) = (
                    NaiveDate::parse_from_str(due_date, "%Y-%m-%d"),
                    NaiveDate::parse_from_str(&today, "%Y-%m-%d")
                ) {
                    if due < today_date {
                        due_label.set_markup(&format!("<span color='#f27835'>Overdue: {}</span>", 
                            due.format("%b %d").to_string()));
                    } else {
                        due_label.set_text(&format!("Due: {}", due.format("%b %d").to_string()));
                    }
                    vbox.append(&due_label);
                }
            }
        }
        
        hbox.append(&vbox);
        
        let delete_button = Button::from_icon_name("user-trash-symbolic");
        delete_button.add_css_class("calendar-todo-delete");
        hbox.append(&delete_button);
        
        row.set_child(Some(&hbox));
        list.append(&row);
        
        // Handle checkbox toggle
        let todo_id = todo.id.clone();
        let label_weak = label.downgrade();
        let todo_store_check = todo_store.clone();
        let current_date_str = current_date.to_string();
        let list_weak = list.downgrade();
        
        check.connect_toggled(move |check| {
            let mut store = todo_store_check.borrow_mut();
            if let Some(completed) = store.toggle_todo(&todo_id) {
                if let Some(label) = label_weak.upgrade() {
                    if completed {
                        label.add_css_class("calendar-todo-completed");
                    } else {
                        label.remove_css_class("calendar-todo-completed");
                    }
                }
                Self::save_todo_store(&store);
                drop(store); // Release the borrow
                
                // Refresh the entire list
                if let Some(list) = list_weak.upgrade() {
                    // Find the parent widgets to refresh
                    let mut current = list.parent();
                    while let Some(widget) = current {
                        if let Some(popover) = widget.downcast_ref::<Popover>() {
                            // Found the popover, now find the active and completed lists
                            if let Some(main_box) = popover.child() {
                                if let Some(main_box) = main_box.downcast_ref::<Box>() {
                                    // Find the lists (they're in ScrolledWindows)
                                    let mut active_list: Option<ListBox> = None;
                                    let mut completed_list: Option<ListBox> = None;
                                    let mut todo_count: Option<Label> = None;
                                    let mut completed_count: Option<Label> = None;
                                    
                                    let mut child = main_box.first_child();
                                    while let Some(widget) = child {
                                        if let Some(scroll) = widget.downcast_ref::<ScrolledWindow>() {
                                            if let Some(list_child) = scroll.child() {
                                                if let Some(list_box) = list_child.downcast_ref::<ListBox>() {
                                                    if active_list.is_none() {
                                                        active_list = Some(list_box.clone());
                                                    } else if completed_list.is_none() {
                                                        completed_list = Some(list_box.clone());
                                                    }
                                                }
                                            }
                                        } else if let Some(hbox) = widget.downcast_ref::<Box>() {
                                            // Check for count labels in header boxes
                                            if let Some(last_child) = hbox.last_child() {
                                                if let Some(label) = last_child.downcast_ref::<Label>() {
                                                    if label.has_css_class("calendar-todo-count") {
                                                        if todo_count.is_none() {
                                                            todo_count = Some(label.clone());
                                                        } else if completed_count.is_none() {
                                                            completed_count = Some(label.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        child = widget.next_sibling();
                                    }
                                    
                                    // Refresh both lists
                                    if let (Some(active), Some(completed), Some(t_count), Some(c_count)) = 
                                        (active_list, completed_list, todo_count, completed_count) {
                                        Self::update_todo_lists(&active, &completed, &current_date_str, 
                                                              &todo_store_check, &t_count, &c_count);
                                    }
                                }
                            }
                            break;
                        }
                        current = widget.parent();
                    }
                }
            }
        });
        
        // Handle delete
        let row_weak_delete = row.downgrade();
        let todo_id_delete = todo.id.clone();
        delete_button.connect_clicked(move |_| {
            let mut store = todo_store.borrow_mut();
            if store.delete_todo(&todo_id_delete) {
                Self::save_todo_store(&store);
                
                if let Some(row) = row_weak_delete.upgrade() {
                    if let Some(parent) = row.parent() {
                        if let Some(list) = parent.downcast_ref::<ListBox>() {
                            list.remove(&row);
                        }
                    }
                }
            }
        });
    }
    
    fn update_time(label: &Label, format: &str) {
        let now = Local::now();
        label.set_text(&now.format(format).to_string());
    }
    
    fn load_todo_store() -> TodoStore {
        match Self::todo_store_path() {
            Ok(path) => {
                info!("Loading todos from: {:?}", path);
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<TodoStore>(&content) {
                            Ok(store) => {
                                info!("Successfully loaded {} todos", store.todos.len());
                                store
                            }
                            Err(e) => {
                                warn!("Failed to parse todos file: {}", e);
                                TodoStore::new()
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            info!("No existing todos file found, creating new store");
                        } else {
                            warn!("Failed to read todos file: {}", e);
                        }
                        TodoStore::new()
                    }
                }
            }
            Err(e) => {
                warn!("Failed to get todo store path: {}", e);
                TodoStore::new()
            }
        }
    }
    
    fn save_todo_store(store: &TodoStore) {
        match Self::todo_store_path() {
            Ok(path) => {
                // Ensure parent directory exists
                if let Some(parent) = path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        warn!("Failed to create data directory: {}", e);
                        return;
                    }
                }
                
                // Serialize the store
                match serde_json::to_string_pretty(store) {
                    Ok(json) => {
                        // Write to a temporary file first for atomic updates
                        let temp_path = path.with_extension("tmp");
                        match fs::write(&temp_path, json) {
                            Ok(_) => {
                                // Rename temp file to actual file (atomic on most filesystems)
                                if let Err(e) = fs::rename(&temp_path, &path) {
                                    warn!("Failed to rename temp file: {}", e);
                                    // Try direct write as fallback
                                    if let Err(e) = fs::write(&path, serde_json::to_string_pretty(store).unwrap_or_default()) {
                                        warn!("Failed to write todos file: {}", e);
                                    }
                                } else {
                                    info!("Successfully saved {} todos", store.todos.len());
                                }
                            }
                            Err(e) => {
                                warn!("Failed to write temp todos file: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to serialize todos: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to get todo store path: {}", e);
            }
        }
    }
    
    fn todo_store_path() -> Result<PathBuf> {
        // Use XDG_DATA_HOME or fallback to ~/.local/share
        let data_dir = dirs::data_local_dir()
            .or_else(|| {
                std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or_else(|| anyhow::anyhow!("Could not determine XDG data directory"))?;
        
        let path = data_dir.join("niri-panel").join("todos.json");
        info!("Todo store path: {:?}", path);
        Ok(path)
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}