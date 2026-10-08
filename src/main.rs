use cosmic::app::{Core, Settings};
use cosmic::iced::{Length, Task};
use cosmic::widget::{text, scrollable, Column, Row, text_input, icon, container};
use cosmic::widget::button;
use cosmic::{Application, Element};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowInfo {
    pub id: String,
    pub app_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct WindowRule {
    appid: String,
    title: String,
    enabled: bool,
}

mod wayland;

fn get_open_windows() -> Vec<WindowInfo> {
    if let Some(state_arc) = wayland::WINDOWS_STATE.get() {
        if let Ok(wins) = state_arc.lock() {
            return wins.values().cloned().collect();
        }
    }
    vec![]
}

fn rules_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        format!("{}/.config", home)
    });
    PathBuf::from(config_dir).join("cosmic/com.system76.CosmicSettings.WindowRules/v1/tiling_exception_custom")
}

fn read_rules() -> Vec<WindowRule> {
    let path = rules_path();
    if let Ok(content) = fs::read_to_string(&path) {
        ron::from_str(&content).unwrap_or_default()
    } else {
        vec![]
    }
}

fn write_rules(rules: &[WindowRule]) -> Result<(), String> {
    let path = rules_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Err(format!("Failed to create directories: {}", e));
        }
    }
    
    let pretty = ron::ser::PrettyConfig::default();
    let content = ron::ser::to_string_pretty(rules, pretty)
        .map_err(|e| format!("Serialization error: {}", e))?;
        
    fs::write(path, content).map_err(|e| format!("Write error: {}", e))
}

struct TilingApp {
    core: Core,
    windows: Vec<WindowInfo>,
    rules: Vec<WindowRule>,
    search_query: String,
    status_message: Option<String>,
}

#[derive(Clone, Debug)]
enum Message {
    AutoRefresh,
    RefreshDone(Vec<WindowInfo>, Vec<WindowRule>),
    AddRule(String),
    RemoveRule(String),
    ToggleRule(String, bool),
    RulesSaved(Result<(), String>),
    SearchChanged(String),
    ExportRules,
    ImportRules,
    RulesImported(Option<Vec<WindowRule>>),
    FileOperationDone,
}

impl Application for TilingApp {
    type Executor = cosmic::iced::executor::Default;
    type Message = Message;
    type Flags = ();

    const APP_ID: &'static str = "com.system76.CosmicTilingManager";

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: ()) -> (Self, Task<cosmic::Action<Message>>) {
        (
            TilingApp {
                core,
                windows: vec![],
                rules: vec![],
                search_query: String::new(),
                status_message: Some("Carregando...".to_string()),
            },
            Task::perform(
                async { (get_open_windows(), read_rules()) },
                |(w, r)| Message::RefreshDone(w, r)
            ).map(Into::into),
        )
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::AutoRefresh)
    }

    fn update(&mut self, message: Message) -> Task<cosmic::Action<Message>> {
        match message {
            Message::AutoRefresh => {
                Task::perform(
                    async { (get_open_windows(), read_rules()) },
                    |(w, r)| Message::RefreshDone(w, r)
                ).map(Into::into)
            }
            Message::RefreshDone(mut windows, mut rules) => {
                windows.sort_by(|a, b| a.app_id.to_lowercase().cmp(&b.app_id.to_lowercase()));
                rules.sort_by(|a, b| a.appid.to_lowercase().cmp(&b.appid.to_lowercase()));
                self.windows = windows;
                self.rules = rules;
                if self.status_message.as_deref() == Some("Carregando...") {
                    self.status_message = None;
                }
                Task::none()
            }
            Message::AddRule(app_id) => {
                if !self.rules.iter().any(|r| r.appid == app_id) {
                    self.rules.push(WindowRule {
                        appid: app_id,
                        title: ".*".to_string(),
                        enabled: true,
                    });
                    
                    self.status_message = Some("Salvando...".to_string());
                    let rules = self.rules.clone();
                    return Task::perform(
                        async move { write_rules(&rules) },
                        Message::RulesSaved
                    ).map(Into::into);
                }
                Task::none()
            }
            Message::RemoveRule(app_id) => {
                self.rules.retain(|r| r.appid != app_id);
                
                self.status_message = Some("Salvando...".to_string());
                let rules = self.rules.clone();
                Task::perform(
                    async move { write_rules(&rules) },
                    Message::RulesSaved
                ).map(Into::into)
            }
            Message::ToggleRule(app_id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.appid == app_id) {
                    rule.enabled = enabled;
                }
                self.status_message = Some("Salvando estado...".to_string());
                let rules = self.rules.clone();
                Task::perform(
                    async move { write_rules(&rules) },
                    Message::RulesSaved
                ).map(Into::into)
            }
            Message::RulesSaved(Ok(())) => {
                self.status_message = Some("Regras salvas com sucesso!".to_string());
                Task::none()
            }
            Message::RulesSaved(Err(e)) => {
                self.status_message = Some(format!("Erro ao salvar: {}", e));
                Task::none()
            }
            Message::SearchChanged(query) => {
                self.search_query = query;
                Task::none()
            }
            Message::ExportRules => {
                self.status_message = Some("Exportando...".to_string());
                let rules = self.rules.clone();
                Task::perform(
                    async move {
                        if let Some(handle) = rfd::AsyncFileDialog::new()
                            .set_title("Export Rules Backup")
                            .set_file_name("tiling_rules.ron")
                            .add_filter("RON", &["ron"])
                            .save_file()
                            .await 
                        {
                            let pretty = ron::ser::PrettyConfig::default();
                            if let Ok(content) = ron::ser::to_string_pretty(&rules, pretty) {
                                let _ = std::fs::write(handle.path(), content);
                            }
                        }
                    },
                    |_| Message::FileOperationDone
                ).map(Into::into)
            }
            Message::ImportRules => {
                self.status_message = Some("Importando...".to_string());
                Task::perform(
                    async {
                        if let Some(handle) = rfd::AsyncFileDialog::new()
                            .set_title("Import Rules Backup")
                            .add_filter("RON", &["ron"])
                            .pick_file()
                            .await
                        {
                            if let Ok(content) = std::fs::read_to_string(handle.path()) {
                                if let Ok(new_rules) = ron::from_str::<Vec<WindowRule>>(&content) {
                                    return Some(new_rules);
                                }
                            }
                        }
                        None
                    },
                    Message::RulesImported
                ).map(Into::into)
            }
            Message::RulesImported(Some(new_rules)) => {
                self.rules = new_rules;
                let rules = self.rules.clone();
                Task::perform(
                    async move { write_rules(&rules) },
                    Message::RulesSaved
                ).map(Into::into)
            }
            Message::RulesImported(None) => {
                self.status_message = None;
                Task::none()
            }
            Message::FileOperationDone => {
                self.status_message = Some("Operação de arquivo concluída.".to_string());
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let mut open_col = Column::new().spacing(15).push(text("Open Windows").size(20));
        
        if self.windows.is_empty() {
            open_col = open_col.push(text("Nenhuma janela detectada no momento.").size(14));
        } else {
            for w in &self.windows {
                open_col = open_col.push(
                    Row::new().spacing(10).align_y(cosmic::iced::Alignment::Center)
                        .push(icon::from_name(w.app_id.as_str()).size(24))
                        .push(text(&w.app_id).width(Length::Fill))
                        .push(button::text("Float").on_press(Message::AddRule(w.app_id.clone())))
                );
            }
        }

        let mut rule_col = Column::new().spacing(15).padding(10)
            .push(text("Active Exceptions").size(20));
            
        let filtered_rules: Vec<_> = self.rules.iter().filter(|r| r.appid.to_lowercase().contains(&self.search_query.to_lowercase())).collect();
        
        if filtered_rules.is_empty() {
            rule_col = rule_col.push(text("Nenhuma exceção cadastrada ou compatível com a busca.").size(14));
        } else {
            for r in filtered_rules {
                rule_col = rule_col.push(
                    Row::new().spacing(10).align_y(cosmic::iced::Alignment::Center)
                        .push(icon::from_name(r.appid.as_str()).size(24))
                        .push(text(&r.appid).width(Length::Fill))
                        .push(
                            cosmic::widget::toggler(r.enabled)
                                .on_toggle({
                                    let id = r.appid.clone();
                                    move |b| Message::ToggleRule(id.clone(), b)
                                })
                        )
                        .push(
                            button::icon(icon::from_name("user-trash-symbolic"))
                                .on_press(Message::RemoveRule(r.appid.clone()))
                        )
                );
            }
        }
        
        let left_panel = container(scrollable(container(open_col).padding(10)))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .padding(20)
            .class(cosmic::theme::Container::Secondary);
            
        let right_panel = container(scrollable(container(rule_col).padding(10)))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .padding(20);

        let content = Row::new().spacing(30)
            .push(left_panel)
            .push(right_panel);
            
        let header_text = Column::new().spacing(5)
            .push(text("COSMIC Tiling Exceptions Manager").size(32))
            .push(text("Manage applications that should bypass the COSMIC tiling system and open in floating mode.").size(16));
            
        let search_box = text_input("Search active rules...", &self.search_query)
            .on_input(Message::SearchChanged)
            .width(Length::Fixed(300.0));
            
        let header = Row::new()
            .spacing(20)
            .align_y(cosmic::iced::Alignment::Center)
            .push(header_text.width(Length::Fill))
            .push(search_box);
            
        let mut toolbar = Row::new()
            .spacing(15)
            .align_y(cosmic::iced::Alignment::Center)
            .push(button::text("Export Backup").on_press(Message::ExportRules))
            .push(button::text("Import Backup").on_press(Message::ImportRules));

        if let Some(msg) = &self.status_message {
            toolbar = toolbar.push(text(msg).size(14));
        }

        Column::new().spacing(20).padding(30)
            .push(header)
            .push(toolbar)
            .push(content)
            .into()
    }
}

fn main() -> cosmic::iced::Result {
    wayland::spawn_listener();
    cosmic::app::run::<TilingApp>(Settings::default(), ())
}
