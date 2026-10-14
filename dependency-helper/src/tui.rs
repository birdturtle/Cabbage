use cabbage_dependency_helper::{probe, write_report, Client, Phase, Plan, Report};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
use std::{
    fs,
    io::{self, IsTerminal},
    path::PathBuf,
    process::Command,
    time::Duration,
};
const GOLD: Color = Color::Rgb(240, 160, 0);
fn draw(frame: &mut Frame, plan: &Plan, install_selected: bool, scroll: u16) {
    frame.render_widget(
        Block::new().style(
            Style::default()
                .fg(Color::Rgb(242, 242, 242))
                .bg(Color::Rgb(25, 25, 25)),
        ),
        frame.area(),
    );
    if frame.area().width < 72 || frame.area().height < 22 {
        frame.render_widget(
            Paragraph::new("Resize this terminal to at least 72 × 22. Esc cancels.")
                .style(Style::default().fg(GOLD))
                .wrap(Wrap { trim: false }),
            frame.area(),
        );
        return;
    }
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(4),
        Constraint::Length(3),
    ])
    .margin(1)
    .split(frame.area());
    frame.render_widget(
        Paragraph::new("Cabbage · Install requirements")
            .style(Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
            .block(Block::bordered().border_style(Style::default().fg(GOLD))),
        areas[0],
    );
    let mut lines = vec![
        Line::from(format!("Client: {}", plan.client.name())),
        Line::from(format!("System: {}", plan.system)),
        Line::from(""),
    ];
    if plan.missing.is_empty() {
        lines.push(Line::from("Requirements are already installed."));
    } else {
        lines.push(Line::from("Missing requirements:"));
        for requirement in &plan.missing {
            lines.push(Line::from(format!("  {}", requirement.name)));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Packages: {}",
            plan.packages().join(", ")
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false })
            .block(Block::bordered()),
        areas[1],
    );
    frame.render_widget(Paragraph::new("This also updates installed system packages.\nPacman will show the full transaction before you approve it.").style(Style::default().fg(GOLD)).wrap(Wrap{trim:false}).block(Block::bordered()),areas[2]);
    let chosen = Style::default()
        .fg(Color::Black)
        .bg(GOLD)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " Install ",
                    if install_selected {
                        chosen
                    } else {
                        Style::default()
                    },
                ),
                Span::raw("   "),
                Span::styled(
                    " Cancel ",
                    if !install_selected {
                        chosen
                    } else {
                        Style::default()
                    },
                ),
            ]),
            Line::from("Tab/←/→ select · ↑/↓ scroll · Enter confirm · Esc cancel"),
        ]),
        areas[3],
    );
}
fn consent(plan: &Plan) -> Result<bool, String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("Open this helper in a terminal.".into());
    }
    let mut terminal = ratatui::init();
    let result = (|| {
        let mut selected = false;
        let mut scroll = 0u16;
        let mut redraw = true;
        loop {
            if redraw {
                terminal
                    .draw(|frame| draw(frame, plan, selected, scroll))
                    .map_err(|_| "Cannot draw setup terminal.")?;
                redraw = false;
            }
            if event::poll(Duration::from_millis(250)).map_err(|_| "Cannot read terminal input.")? {
                let input = event::read().map_err(|_| "Cannot read terminal input.")?;
                if matches!(input, Event::Resize(..)) {
                    redraw = true;
                }
                if let Event::Key(key) = input {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    redraw = true;
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => return Ok(false),
                        KeyCode::Char('c')
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                        {
                            return Ok(false)
                        }
                        KeyCode::Down => {
                            scroll = scroll.saturating_add(1).min(plan.missing.len() as u16 + 5)
                        }
                        KeyCode::Up => scroll = scroll.saturating_sub(1),
                        KeyCode::Tab | KeyCode::Left | KeyCode::Right => selected = !selected,
                        KeyCode::Enter => {
                            let size = terminal.size().map_err(|_| "Cannot read terminal size.")?;
                            if size.width >= 72 && size.height >= 22 {
                                return Ok(selected && !plan.missing.is_empty());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    })();
    ratatui::restore();
    result
}
fn clean_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("JX_") {
            command.env_remove(name);
        }
    }
    for name in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "APPIMAGE",
        "APPDIR",
        "ARGV0",
    ] {
        command.env_remove(name);
    }
}
pub fn run() -> Result<(), String> {
    let inspection: Vec<_> = std::env::args().skip(1).collect();
    if inspection.len() == 2 && inspection[0] == "--check" {
        let client = Client::parse(&inspection[1])?;
        println!(
            "{}",
            serde_json::to_string_pretty(&probe(client))
                .map_err(|_| "Cannot encode requirements.")?
        );
        return Ok(());
    }
    if unsafe { libc::geteuid() } == 0 {
        return Err("Run this helper as your normal user. Sudo will ask when needed.".into());
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let preview = args.len() == 1 && args[0] == "--preview";
    let (client, session) = if preview {
        (Client::OfficialOsrs, None)
    } else {
        if args.len() != 3 {
            return Err("Invalid setup invocation.".into());
        }
        let client = Client::parse(args[0].to_str().ok_or("Invalid client.")?)?;
        let path = PathBuf::from(&args[1]);
        let token = args[2].to_str().ok_or("Invalid setup token.")?.to_string();
        if !path.is_absolute()
            || path.file_name().and_then(|n| n.to_str()) != Some("status.json")
            || token.len() != 64
            || !token.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid setup session.".into());
        }
        use std::os::unix::fs::MetadataExt;
        let parent = path.parent().ok_or("Invalid setup folder.")?;
        let metadata = fs::symlink_metadata(parent).map_err(|_| "Missing setup folder.")?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err("Setup folder must be private and owned by your user.".into());
        }
        (client, Some((path, token)))
    };
    let report = |phase, message: &str| -> Result<(), String> {
        if let Some((path, token)) = &session {
            write_report(
                path,
                &Report {
                    token: token.clone(),
                    pid: std::process::id(),
                    phase,
                    message: message.into(),
                },
            )?;
        }
        Ok(())
    };
    report(Phase::Waiting, "Finish setup in the terminal.")?;
    let result = (|| {
        let plan = if preview {
            Plan {
                client,
                system: "CachyOS (preview)".into(),
                supported: true,
                missing: vec![cabbage_dependency_helper::Requirement {
                    name: "Wine".into(),
                    package: "wine".into(),
                }],
            }
        } else {
            probe(client)
        };
        if !plan.supported {
            return Err("Automatic setup currently supports Arch and CachyOS x64 only.".into());
        }
        if plan.missing.is_empty() {
            report(Phase::Finished, "Requirements are already installed.")?;
            return Ok(());
        }
        if !consent(&plan)? {
            report(Phase::Cancelled, "Setup cancelled. Nothing was installed.")?;
            return Ok(());
        }
        if preview {
            println!("Preview only. No packages were installed.");
            return Ok(());
        }
        if PathBuf::from("/var/lib/pacman/db.lck").exists() {
            return Err("Another package transaction is running. Finish it and try again.".into());
        }
        let live = probe(client);
        if live.missing.is_empty() {
            report(Phase::Finished, "Requirements are already installed.")?;
            return Ok(());
        }
        if live.packages().iter().any(|p| !plan.packages().contains(p)) {
            return Err(
                "Requirements changed during review. Reopen setup to review the new packages."
                    .into(),
            );
        }
        let arguments = live.arguments()?;
        report(
            Phase::Installing,
            "Installing requirements in the terminal…",
        )?;
        println!("\x1b[38;2;240;160;0mCabbage · Installing requirements\x1b[0m\nKeep this terminal open until installation finishes.\n");
        let mut command = Command::new("/usr/bin/sudo");
        command.args(arguments);
        clean_environment(&mut command);
        let exit = command
            .status()
            .map_err(|_| "Cannot start sudo and pacman.")?;
        if !exit.success() {
            return Err("Package installation did not finish. Check the terminal output and retry when ready.".into());
        }
        let remaining = probe(client);
        if !remaining.missing.is_empty() {
            return Err(format!(
                "Setup finished, but these requirements are still missing: {}.",
                remaining
                    .missing
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        report(Phase::Finished, "Requirements installed. Ready to play.")?;
        println!("\n\x1b[38;2;240;160;0mRequirements installed. Return to Cabbage.\x1b[0m");
        Ok(())
    })();
    if let Err(error) = &result {
        let _ = report(Phase::Failed, error);
        eprintln!("\n{error}");
    }
    if session.is_some() && io::stdin().is_terminal() {
        println!("\nPress Enter to close.");
        let _ = io::stdin().read_line(&mut String::new());
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_terminal_cannot_hide_the_review_behind_install_controls() {
        let plan = Plan {
            client: Client::OfficialOsrs,
            system: "CachyOS".into(),
            supported: true,
            missing: vec![cabbage_dependency_helper::Requirement {
                name: "Wine".into(),
                package: "wine".into(),
            }],
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 10)).unwrap();
        terminal.draw(|frame| draw(frame, &plan, true, 0)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Resize this terminal"));
        assert!(!text.contains(" Install "));
    }
    #[test]
    fn preview_shows_packages_and_system_update_before_consent() {
        let plan = Plan {
            client: Client::OfficialOsrs,
            system: "CachyOS".into(),
            supported: true,
            missing: vec![cabbage_dependency_helper::Requirement {
                name: "Wine".into(),
                package: "wine".into(),
            }],
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &plan, false, 0)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("wine"));
        assert!(text.contains("updates installed system packages"));
        assert!(text.contains("Cancel"));
    }
}
