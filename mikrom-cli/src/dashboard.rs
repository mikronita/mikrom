use crate::client::{MikromClient, VmStatusResponse};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};
use std::io;
use std::time::{Duration, Instant};

pub async fn run(client: MikromClient) -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(client);

    // Run loop
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{err:?}");
    }

    Ok(())
}

struct App {
    client: MikromClient,
    vms: Vec<VmStatusResponse>,
    state: TableState,
    last_tick: Instant,
}

impl App {
    fn new(client: MikromClient) -> App {
        App {
            client,
            vms: Vec::new(),
            state: TableState::default(),
            last_tick: Instant::now(),
        }
    }

    async fn tick(&mut self) -> anyhow::Result<()> {
        if let Ok(vms) = self.client.list_vms().await {
            // Update the basic list
            let mut updated_vms = Vec::new();
            for basic_vm in vms {
                // Fetch full status including metrics for each VM
                if let Ok(full_status) = self.client.get_vm(&basic_vm.job_id).await {
                    updated_vms.push(full_status);
                } else {
                    // Fallback to basic info if full status fails
                    updated_vms.push(VmStatusResponse {
                        job_id: basic_vm.job_id,
                        status: basic_vm.status,
                        host_id: basic_vm.host_id,
                        vm_id: basic_vm.vm_id,
                        scheduled_at: 0,
                        started_at: 0,
                        stopped_at: 0,
                        error_message: String::new(),
                        cpu_usage: 0.0,
                        ram_used_bytes: 0,
                    });
                }
            }
            self.vms = updated_vms;
        }
        Ok(())
    }

    pub fn next(&mut self) {
        if self.vms.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.vms.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.vms.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.vms.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> anyhow::Result<()>
where
    B::Error: std::fmt::Debug + Send + Sync + 'static,
{
    let tick_rate = Duration::from_secs(2);

    // Initial fetch
    let _ = app.tick().await;

    loop {
        terminal
            .draw(|f| ui(f, app))
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                _ => {}
            }
        }

        if app.last_tick.elapsed() >= tick_rate {
            let _ = app.tick().await;
            app.last_tick = Instant::now();
        }
    }
}

fn ui(f: &mut ratatui::Frame<'_>, app: &mut App) {
    let rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0)].as_ref())
        .split(f.area());

    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let normal_style = Style::default().bg(Color::Blue);
    let header_cells = ["Job ID", "Status", "Instance ID", "CPU (%)", "RAM (MiB)"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
    let header = Row::new(header_cells)
        .style(normal_style)
        .height(1)
        .bottom_margin(1);

    let rows = app.vms.iter().map(|item| {
        let status_color = match item.status.as_str() {
            "Running" => Color::Green,
            "Scheduled" | "Pending" => Color::Yellow,
            "Failed" | "Error" => Color::Red,
            _ => Color::White,
        };

        let cpu = format!("{:.1}%", item.cpu_usage * 100.0);
        let ram_mib = (item.ram_used_bytes as f64) / (1024.0 * 1024.0);
        let ram = format!("{ram_mib:.1} MiB");

        let cells = vec![
            Cell::from(item.job_id.clone()),
            Cell::from(item.status.clone()).style(Style::default().fg(status_color)),
            Cell::from(item.vm_id.clone()),
            Cell::from(cpu),
            Cell::from(ram),
        ];
        Row::new(cells).height(1)
    });

    let t = Table::new(
        rows,
        [
            Constraint::Length(38),
            Constraint::Length(12),
            Constraint::Length(38),
            Constraint::Length(10),
            Constraint::Length(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Mikrom Dashboard (Press 'q' to quit) "),
    )
    .row_highlight_style(selected_style)
    .highlight_symbol(">> ");

    f.render_stateful_widget(t, rects[0], &mut app.state);
}
