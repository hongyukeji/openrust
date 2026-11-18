mod device;

use std::{collections::VecDeque, fmt::Write as _, io, sync::Arc, time::SystemTime};

use console::{measure_text_width, strip_ansi_codes, truncate_str};

use crate::foundation::{DevCommandMode, dev_commands::RegisteredProcess};

use super::dev_command::Event;
use device::{Device, Key};

pub(super) enum Action {
    Continue,
    Restart(usize),
    Quit,
}

pub(super) struct Terminal {
    device: Device,
    view: View,
    dirty: bool,
    size: (u16, u16),
}

impl Terminal {
    pub(super) fn open(
        mode: DevCommandMode,
        processes: &[RegisteredProcess],
        limit: usize,
        stream_limit: usize,
        timestamps: bool,
        decorated: bool,
    ) -> Option<Self> {
        let size = Device::size().filter(|&(rows, columns)| rows >= 8 && columns >= 40)?;
        Some(Self {
            device: Device::open().ok()?,
            view: View::new(mode, processes, limit, stream_limit, timestamps, decorated),
            dirty: true,
            size,
        })
    }

    pub(super) async fn action(terminal: &mut Option<Self>) -> Action {
        let Some(terminal) = terminal else {
            return std::future::pending().await;
        };
        let key = terminal.device.key().await;
        terminal.dirty = true;
        terminal.view.key(key, terminal.size.0)
    }

    pub(super) fn record(&mut self, index: usize, event: Event) {
        self.view.record(Some(index), event);
        self.dirty = true;
    }

    pub(super) fn message(&mut self, message: String) {
        self.view.record(None, Event::Error { message });
        self.dirty = true;
    }

    pub(super) fn draw(&mut self) -> io::Result<()> {
        let size = Device::size().unwrap_or(self.size);
        if self.dirty || self.size != size {
            self.device.write(&self.view.frame(size))?;
            self.size = size;
            self.dirty = false;
        }
        Ok(())
    }

    pub(super) fn finish(self, mut replay: impl FnMut(Option<usize>, Event, &str)) {
        #[cfg(unix)]
        drop(self.device);
        for line in self.view.stream {
            replay(line.index, line.event.clone(), &line.time);
        }
    }
}

struct ProcessPane {
    name: String,
    color: String,
    status: &'static str,
    logs: VecDeque<Arc<Line>>,
    visible: bool,
}

struct Line {
    index: Option<usize>,
    event: Event,
    text: String,
    time: String,
}

enum Prompt {
    None,
    Search,
    Filter,
}

struct View {
    panes: Vec<ProcessPane>,
    stream: VecDeque<Arc<Line>>,
    mode: DevCommandMode,
    selected: usize,
    sidebar: bool,
    scroll: usize,
    limit: usize,
    stream_limit: usize,
    timestamps: bool,
    decorated: bool,
    prompt: Prompt,
    query: String,
    matched: Option<Arc<Line>>,
}

impl View {
    fn new(
        mode: DevCommandMode,
        processes: &[RegisteredProcess],
        limit: usize,
        stream_limit: usize,
        timestamps: bool,
        decorated: bool,
    ) -> Self {
        let panes = processes
            .iter()
            .map(|process| ProcessPane {
                name: plain(&process.definition.name),
                color: process.definition.color.clone(),
                status: "waiting",
                logs: VecDeque::new(),
                visible: true,
            })
            .collect();
        Self {
            panes,
            stream: VecDeque::new(),
            mode,
            selected: 0,
            sidebar: false,
            scroll: 0,
            limit,
            stream_limit,
            timestamps,
            decorated,
            prompt: Prompt::None,
            query: String::new(),
            matched: None,
        }
    }

    fn record(&mut self, index: Option<usize>, mut event: Event) {
        if let Some(index) = index {
            let pane = &mut self.panes[index];
            pane.status = match event {
                Event::Start { .. } => "running",
                Event::Exit { .. } => "exited",
                Event::Restarting { .. } => "restarting",
                Event::Failed { .. } => "failed",
                _ => pane.status,
            };
        }
        let text = match &mut event {
            Event::Output { text, .. } => {
                *text = plain(text);
                text.clone()
            }
            Event::Error { message } => {
                *message = plain(message);
                message.clone()
            }
            Event::Restarting { attempt, max } => format!("Restarting ({attempt}/{max})"),
            Event::Failed { code, reason } => format!("Process exited with code {code} ({reason})"),
            _ => return,
        };
        let visible = index.is_none_or(|index| match self.mode {
            DevCommandMode::Tabs => index == self.selected,
            _ => self.panes[index].visible,
        });
        if self.scroll > 0 && visible {
            self.scroll = self.scroll.saturating_add(1);
        }
        let time = if self.timestamps {
            chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
                .format("%H:%M:%S ")
                .to_string()
        } else {
            String::new()
        };
        let line = Arc::new(Line {
            index,
            event,
            text,
            time,
        });
        if let Some(index) = index {
            push(&mut self.panes[index].logs, self.limit, line.clone());
        }
        push(&mut self.stream, self.stream_limit, line);
    }

    fn key(&mut self, key: Key, rows: u16) -> Action {
        if matches!(key, Key::Quit) {
            return Action::Quit;
        }
        let page = usize::from(rows).saturating_sub(3).max(1);
        match self.prompt {
            Prompt::Search => {
                match key {
                    Key::Escape => {
                        self.prompt = Prompt::None;
                        self.query.clear();
                        self.matched = None;
                    }
                    Key::Enter => {
                        self.prompt = Prompt::None;
                        self.search(false, page);
                    }
                    Key::Backspace => {
                        self.query.pop();
                    }
                    Key::Character(character)
                        if !character.is_control() && self.query.len() < 256 =>
                    {
                        self.query.push(character)
                    }
                    _ => {}
                }
                return Action::Continue;
            }
            Prompt::Filter => {
                match key {
                    Key::Escape | Key::Enter | Key::Character('f') => self.prompt = Prompt::None,
                    Key::Character(character) => {
                        if let Some(index) =
                            digit(character).filter(|&index| index < self.panes.len())
                        {
                            self.toggle(index);
                        }
                    }
                    _ => {}
                }
                return Action::Continue;
            }
            Prompt::None => {}
        }
        match key {
            Key::Character('q') => return Action::Quit,
            Key::Character('r') if self.mode == DevCommandMode::Tabs => {
                return Action::Restart(self.selected);
            }
            Key::Character('s') => self.switch(DevCommandMode::Stream),
            Key::Character('t') => self.switch(DevCommandMode::Tabs),
            Key::Character('f') if self.mode == DevCommandMode::Stream => {
                self.prompt = Prompt::Filter
            }
            Key::Character('/') => {
                self.prompt = Prompt::Search;
                self.query.clear();
                self.matched = None;
            }
            Key::Character('n') => self.search(false, page),
            Key::Character('N') => self.search(true, page),
            Key::Character('c') => {
                if self.mode == DevCommandMode::Tabs {
                    self.panes[self.selected].logs.clear();
                } else {
                    self.stream.clear();
                }
                self.scroll = 0;
                self.matched = None;
            }
            Key::Tab if self.mode == DevCommandMode::Tabs => self.sidebar = !self.sidebar,
            Key::Left if self.mode == DevCommandMode::Tabs => self.sidebar = true,
            Key::Right if self.mode == DevCommandMode::Tabs => self.sidebar = false,
            Key::Up | Key::Character('k') if self.mode == DevCommandMode::Tabs && self.sidebar => {
                self.select(self.selected.saturating_sub(1))
            }
            Key::Down | Key::Character('j')
                if self.mode == DevCommandMode::Tabs && self.sidebar =>
            {
                self.select((self.selected + 1).min(self.panes.len() - 1))
            }
            Key::Up | Key::Character('k') => self.scroll = self.scroll.saturating_add(1),
            Key::Down | Key::Character('j') => self.scroll = self.scroll.saturating_sub(1),
            Key::PageUp => self.scroll = self.scroll.saturating_add(page),
            Key::PageDown => self.scroll = self.scroll.saturating_sub(page),
            Key::End | Key::Character('G') => self.scroll = 0,
            Key::Home | Key::Character('g') => {
                self.scroll = self.lines().len().saturating_sub(page)
            }
            Key::Character(character) if self.mode == DevCommandMode::Tabs => {
                if let Some(index) = digit(character).filter(|&index| index < self.panes.len()) {
                    self.select(index);
                }
            }
            _ => {}
        }
        self.scroll = self.scroll.min(self.lines().len().saturating_sub(page));
        Action::Continue
    }

    fn select(&mut self, index: usize) {
        self.selected = index;
        self.scroll = 0;
        self.matched = None;
    }

    fn switch(&mut self, mode: DevCommandMode) {
        if self.mode != mode {
            self.mode = mode;
            self.select(self.selected);
        }
    }

    fn toggle(&mut self, index: usize) {
        let pane = &self.panes[index];
        if !pane.visible || self.panes.iter().filter(|pane| pane.visible).count() > 1 {
            self.panes[index].visible = !pane.visible;
        }
        self.scroll = 0;
        self.matched = None;
    }

    fn lines(&self) -> Vec<&Arc<Line>> {
        if self.mode == DevCommandMode::Tabs {
            self.panes[self.selected].logs.iter().collect()
        } else {
            self.stream
                .iter()
                .filter(|line| line.index.is_none_or(|index| self.panes[index].visible))
                .collect()
        }
    }

    fn search(&mut self, backwards: bool, page: usize) {
        if self.query.is_empty() {
            return;
        }
        let lines = self.lines();
        let previous = self
            .matched
            .as_ref()
            .and_then(|matched| lines.iter().position(|line| Arc::ptr_eq(line, matched)));
        let candidates: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| line.text.contains(&self.query).then_some(index))
            .collect();
        let found = if backwards {
            candidates
                .iter()
                .rev()
                .find(|&&index| previous.is_none_or(|from| index < from))
                .or(candidates.last())
        } else {
            candidates
                .iter()
                .find(|&&index| previous.is_none_or(|from| index > from))
                .or(candidates.first())
        }
        .copied();
        if let Some(index) = found {
            let max_scroll = lines.len().saturating_sub(page);
            let line = Arc::clone(lines[index]);
            self.scroll = lines
                .len()
                .saturating_sub(index + page / 2 + 1)
                .min(max_scroll);
            self.matched = Some(line);
        }
    }

    fn frame(&self, (rows, columns): (u16, u16)) -> String {
        let columns = usize::from(columns);
        let rows = usize::from(rows);
        let mut frame = String::from("\x1b[H");
        if rows < 5 || columns < 20 {
            row(&mut frame, "artisan dev · q quit", columns);
            frame.push_str("\x1b[J");
            return frame;
        }
        let tabs = self.mode == DevCommandMode::Tabs;
        let mode = if tabs { "Tabs" } else { "Stream" };
        let heading = if tabs {
            let pane = &self.panes[self.selected];
            format!("artisan dev · {mode} · {} ({})", pane.name, pane.status)
        } else {
            format!("artisan dev · {mode}")
        };
        row(&mut frame, &heading, columns);

        let lines = self.lines();
        let height = rows - 3;
        let end = lines
            .len()
            .saturating_sub(self.scroll.min(lines.len().saturating_sub(height)));
        let start = end.saturating_sub(height);
        let pane_width = (columns / 3).clamp(9, 27);
        let pane_start = self.selected.saturating_sub(height.saturating_sub(1));
        for index in 0..height {
            if tabs {
                let pane_index = index + pane_start;
                let text = self.panes.get(pane_index).map_or_else(String::new, |pane| {
                    let point = if pane_index == self.selected {
                        if self.sidebar { "»" } else { ">" }
                    } else {
                        " "
                    };
                    format!("{point}{} {} [{}]", pane_index + 1, pane.name, pane.status)
                });
                let prefix = truncate_str(&text, pane_width - 1, "…");
                frame.push_str(&prefix);
                let padding = pane_width.saturating_sub(measure_text_width(&prefix));
                let _ = write!(frame, "{:padding$}│ ", "");
            }
            let width = if tabs {
                columns - pane_width - 2
            } else {
                columns
            };
            let position = start + index;
            if let Some(line) = lines.get(position).filter(|_| position < end) {
                self.draw_line(
                    &mut frame,
                    line,
                    width,
                    !tabs,
                    self.matched
                        .as_ref()
                        .is_some_and(|matched| Arc::ptr_eq(line, matched)),
                );
            } else {
                row(&mut frame, "", width);
            }
        }
        let footer = match self.prompt {
            Prompt::Search => format!("Search: {}_", self.query),
            Prompt::Filter => self
                .panes
                .iter()
                .enumerate()
                .map(|(index, pane)| {
                    let mark = if pane.visible { "+" } else { "-" };
                    format!("{}:{mark}{}", index + 1, pane.name)
                })
                .collect::<Vec<_>>()
                .join(" "),
            Prompt::None if !self.query.is_empty() => {
                format!("Search: {} · n/N next/previous", self.query)
            }
            Prompt::None if tabs => {
                "1–9 select · Tab focus · s stream · r restart · / search · q quit".to_owned()
            }
            Prompt::None => "t tabs · f filter · / search · c clear · q quit".to_owned(),
        };
        row(&mut frame, &footer, columns);
        frame.push_str("\x1b[J");
        frame
    }

    fn draw_line(
        &self,
        frame: &mut String,
        line: &Line,
        width: usize,
        labels: bool,
        matched: bool,
    ) {
        let pane = line.index.map(|index| &self.panes[index]);
        let label = if labels {
            pane.map_or_else(String::new, |pane| format!("[{}] ", pane.name))
        } else {
            String::new()
        };
        let marker = if matched { "› " } else { "" };
        let text = format!("{marker}{}{label}{}", line.time, line.text);
        if self.decorated {
            if matched {
                frame.push_str("\x1b[7m");
            } else if let Some(rgb) = pane
                .and_then(|pane| pane.color.strip_prefix('#'))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            {
                let _ = write!(
                    frame,
                    "\x1b[38;2;{};{};{}m",
                    (rgb >> 16) & 255,
                    (rgb >> 8) & 255,
                    rgb & 255
                );
            }
        }
        frame.push_str(&truncate_str(&text, width, "…"));
        frame.push_str("\x1b[0m\x1b[K\r\n");
    }
}

fn plain(value: &str) -> String {
    strip_ansi_codes(value)
        .chars()
        .take(4096)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn push(log: &mut VecDeque<Arc<Line>>, limit: usize, line: Arc<Line>) {
    if limit == 0 {
        return;
    }
    if log.len() == limit {
        log.pop_front();
    }
    log.push_back(line);
}

fn row(frame: &mut String, text: &str, width: usize) {
    frame.push_str(&truncate_str(text, width, "…"));
    frame.push_str("\x1b[K\r\n");
}

fn digit(character: char) -> Option<usize> {
    character
        .to_digit(10)
        .and_then(|digit| digit.checked_sub(1))
        .map(|index| index as usize)
}
