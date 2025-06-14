use gtk4::prelude::*;
use gtk4::{Label, Button, Popover, Box, Orientation, Calendar, ScrolledWindow, TextView, TextBuffer, Entry, ListBox, ListBoxRow, CheckButton, ApplicationWindow};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use glib::timeout_add_seconds_local;
use chrono::{Local, Datelike};
use anyhow::Result;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DayNote {
    note: String,
    todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    text: String,
    completed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CalendarData {
    notes: HashMap<String, DayNote>,
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
        main_box.set_size_request(400, -1);
        
        // Calendar widget
        let calendar = Calendar::new();
        calendar.add_css_class("clock-calendar");
        main_box.append(&calendar);
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        main_box.append(&separator);
        
        // Notes section
        let notes_label = Label::new(Some("Notes for selected date"));
        notes_label.set_halign(gtk4::Align::Start);
        notes_label.add_css_class("calendar-section-label");
        main_box.append(&notes_label);
        
        // Note text view
        let note_scroll = ScrolledWindow::new();
        note_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        note_scroll.set_min_content_height(80);
        note_scroll.set_max_content_height(120);
        
        let note_buffer = TextBuffer::new(None);
        let note_view = TextView::with_buffer(&note_buffer);
        note_view.add_css_class("calendar-note");
        note_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        note_view.set_left_margin(5);
        note_view.set_right_margin(5);
        note_view.set_top_margin(5);
        note_view.set_bottom_margin(5);
        
        note_scroll.set_child(Some(&note_view));
        main_box.append(&note_scroll);
        
        // Todo section
        let todo_label = Label::new(Some("Todo items"));
        todo_label.set_halign(gtk4::Align::Start);
        todo_label.add_css_class("calendar-section-label");
        todo_label.set_margin_top(10);
        main_box.append(&todo_label);
        
        // Todo input
        let todo_box = Box::new(Orientation::Horizontal, 5);
        let todo_entry = Entry::new();
        todo_entry.set_placeholder_text(Some("Add a todo item..."));
        todo_entry.set_hexpand(true);
        todo_entry.add_css_class("calendar-todo-entry");
        
        let add_button = Button::with_label("Add");
        add_button.add_css_class("calendar-todo-add");
        
        todo_box.append(&todo_entry);
        todo_box.append(&add_button);
        main_box.append(&todo_box);
        
        // Todo list
        let todo_scroll = ScrolledWindow::new();
        todo_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        todo_scroll.set_min_content_height(100);
        todo_scroll.set_max_content_height(200);
        
        let todo_list = ListBox::new();
        todo_list.add_css_class("calendar-todo-list");
        todo_list.set_selection_mode(gtk4::SelectionMode::None);
        
        todo_scroll.set_child(Some(&todo_list));
        main_box.append(&todo_scroll);
        
        popover.set_child(Some(&main_box));
        
        // Load calendar data
        let calendar_data = Rc::new(RefCell::new(Self::load_calendar_data()));
        
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
        let calendar_data_clone = calendar_data.clone();
        let note_buffer_weak = note_buffer.downgrade();
        let todo_list_weak = todo_list.downgrade();
        calendar.connect_day_selected(move |cal| {
            let date = format!("{}-{:02}-{:02}", 
                cal.year(), cal.month() + 1, cal.day());
            
            if let (Some(buffer), Some(list)) = (note_buffer_weak.upgrade(), todo_list_weak.upgrade()) {
                let data = calendar_data_clone.borrow();
                
                // Update note
                if let Some(day_note) = data.notes.get(&date) {
                    buffer.set_text(&day_note.note);
                    
                    // Update todo list
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                    
                    for (idx, todo) in day_note.todos.iter().enumerate() {
                        Self::add_todo_item(&list, todo, idx, &date, calendar_data_clone.clone());
                    }
                } else {
                    buffer.set_text("");
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                }
            }
        });
        
        // Handle note changes
        let calendar_data_for_note = calendar_data.clone();
        let calendar_weak = calendar.downgrade();
        note_buffer.connect_changed(move |buffer| {
            if let Some(cal) = calendar_weak.upgrade() {
                let date = format!("{}-{:02}-{:02}", 
                    cal.year(), cal.month() + 1, cal.day());
                
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let mut data = calendar_data_for_note.borrow_mut();
                
                if text.is_empty() {
                    if let Some(day_note) = data.notes.get_mut(&date) {
                        day_note.note.clear();
                        if day_note.todos.is_empty() {
                            data.notes.remove(&date);
                        }
                    }
                } else {
                    data.notes.entry(date).or_insert_with(|| DayNote {
                        note: String::new(),
                        todos: Vec::new(),
                    }).note = text.to_string();
                }
                
                Self::save_calendar_data(&data);
            }
        });
        
        // Handle todo addition
        let calendar_data_for_todo = calendar_data.clone();
        let calendar_for_todo = calendar.downgrade();
        let todo_list_for_add = todo_list.downgrade();
        let todo_entry_weak = todo_entry.downgrade();
        let add_todo = move || {
            if let (Some(cal), Some(list), Some(entry)) = 
                (calendar_for_todo.upgrade(), todo_list_for_add.upgrade(), todo_entry_weak.upgrade()) {
                
                let text = entry.text();
                if !text.is_empty() {
                    let date = format!("{}-{:02}-{:02}", 
                        cal.year(), cal.month() + 1, cal.day());
                    
                    let mut data = calendar_data_for_todo.borrow_mut();
                    let day_note = data.notes.entry(date.clone()).or_insert_with(|| DayNote {
                        note: String::new(),
                        todos: Vec::new(),
                    });
                    
                    let todo = TodoItem {
                        text: text.to_string(),
                        completed: false,
                    };
                    
                    let idx = day_note.todos.len();
                    day_note.todos.push(todo.clone());
                    
                    Self::save_calendar_data(&data);
                    drop(data); // Release borrow before adding to UI
                    
                    Self::add_todo_item(&list, &todo, idx, &date, calendar_data_for_todo.clone());
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
        button.connect_clicked(move |_| {
            // Set calendar to current date
            if let Some(cal) = calendar_for_show.upgrade() {
                let now = Local::now();
                let datetime = gtk4::glib::DateTime::from_local(
                    now.year(),
                    now.month() as i32,
                    now.day() as i32,
                    0, 0, 0.0
                ).unwrap();
                cal.select_day(&datetime);
            }
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn add_todo_item(list: &ListBox, todo: &TodoItem, idx: usize, date: &str, calendar_data: Rc<RefCell<CalendarData>>) {
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
        
        let label = Label::new(Some(&todo.text));
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        if todo.completed {
            label.add_css_class("calendar-todo-completed");
        }
        hbox.append(&label);
        
        let delete_button = Button::from_icon_name("user-trash-symbolic");
        delete_button.add_css_class("calendar-todo-delete");
        hbox.append(&delete_button);
        
        row.set_child(Some(&hbox));
        list.append(&row);
        
        // Handle checkbox toggle
        let label_weak = label.downgrade();
        let date_clone = date.to_string();
        let calendar_data_check = calendar_data.clone();
        check.connect_toggled(move |check| {
            let mut data = calendar_data_check.borrow_mut();
            if let Some(day_note) = data.notes.get_mut(&date_clone) {
                if let Some(todo) = day_note.todos.get_mut(idx) {
                    todo.completed = check.is_active();
                    
                    if let Some(label) = label_weak.upgrade() {
                        if todo.completed {
                            label.add_css_class("calendar-todo-completed");
                        } else {
                            label.remove_css_class("calendar-todo-completed");
                        }
                    }
                    
                    Self::save_calendar_data(&data);
                }
            }
        });
        
        // Handle delete
        let row_weak = row.downgrade();
        let date_for_delete = date.to_string();
        delete_button.connect_clicked(move |_| {
            let mut data = calendar_data.borrow_mut();
            if let Some(day_note) = data.notes.get_mut(&date_for_delete) {
                day_note.todos.remove(idx);
                
                // Remove day entry if empty
                if day_note.note.is_empty() && day_note.todos.is_empty() {
                    data.notes.remove(&date_for_delete);
                }
                
                Self::save_calendar_data(&data);
                
                if let Some(row) = row_weak.upgrade() {
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
    
    fn load_calendar_data() -> CalendarData {
        if let Ok(path) = Self::calendar_data_path() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str(&content) {
                    return data;
                }
            }
        }
        CalendarData::default()
    }
    
    fn save_calendar_data(data: &CalendarData) {
        if let Ok(path) = Self::calendar_data_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, serde_json::to_string_pretty(data).unwrap_or_default());
        }
    }
    
    fn calendar_data_path() -> Result<PathBuf> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find local data directory"))?;
        Ok(data_dir.join("niri-panel").join("calendar.json"))
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}