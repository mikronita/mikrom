use clap::{Subcommand, ValueEnum};

pub fn parse_memory_choice(value: &str) -> Result<u32, String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "512M" => Ok(512),
        "1G" => Ok(1024),
        "2G" => Ok(2048),
        "4G" => Ok(4096),
        _ => Err("memory must be one of 512M, 1G, 2G, or 4G".to_string()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Authentication and Profile
    #[command(subcommand)]
    Auth(AuthCommands),
    /// Application Management
    #[command(subcommand)]
    App(AppCommands),
    /// Live Instance Management (Jobs)
    #[command(subcommand)]
    Deployment(DeploymentCommands),
    /// CLI Configuration
    #[command(subcommand)]
    Config(ConfigCommands),
    /// Persistent Storage Management
    #[command(subcommand)]
    Volume(VolumeCommands),
    /// System Status
    #[command(subcommand)]
    System(SystemCommands),
}

#[derive(Subcommand, Debug)]
pub enum VolumeCommands {
    /// List all volumes (optional filter by application)
    List {
        #[arg(long, short, help = "Name of the application")]
        app: Option<String>,
    },
    /// Create a new persistent volume
    Create {
        #[arg(long, short, help = "Display name for the volume")]
        name: String,
        #[arg(long, short, help = "Size in MiB")]
        size: i32,
    },
    /// Attach a volume to an application
    Attach {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "Volume ID")]
        volume_id: String,
        #[arg(long, short, help = "Mount point inside the VM (e.g. /data)")]
        mount: String,
        #[arg(
            long,
            short = 'A',
            help = "Access mode: 0 (RWO), 1 (RWX), 2 (ROX)",
            default_value = "0"
        )]
        mode: i32,
    },
    /// Detach a volume from an application
    Detach {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "Volume ID")]
        volume_id: String,
    },
    /// Create a snapshot of a volume
    Snapshot {
        #[arg(long, short, help = "Volume ID")]
        volume_id: String,
        #[arg(long, short, help = "Snapshot name")]
        name: String,
    },
    /// Restore a volume to a specific snapshot
    Restore {
        #[arg(long, short, help = "Volume ID")]
        volume_id: String,
        #[arg(long, short, help = "Snapshot name to restore")]
        snapshot: String,
    },
    /// Delete a volume
    Delete {
        #[arg(long, short, help = "Volume ID")]
        volume_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Login with email and password
    Login {
        #[arg(long, short, help = "Account email address")]
        email: String,
        #[arg(long, short, help = "Account password")]
        password: String,
    },
    /// Register a new account
    Register {
        #[arg(long, short, help = "Account email address")]
        email: String,
        #[arg(long, short, help = "Account password")]
        password: String,
    },
    /// Display current user profile
    Whoami,
    /// Update user profile details
    Update {
        #[arg(long, help = "New first name")]
        first_name: Option<String>,
        #[arg(long, help = "New last name")]
        last_name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AppCommands {
    /// List all applications
    List,
    /// Create a new application
    Create {
        #[arg(long, short, help = "Unique name for the application")]
        name: String,
        #[arg(long, short, help = "Git repository URL")]
        git_url: String,
    },
    /// Delete an application
    Delete {
        #[arg(long, short, help = "Name of the application to delete")]
        name: String,
    },
    /// Deploy a new version of an application (triggers build)
    Deploy {
        #[arg(long, short, help = "Name of the application to deploy")]
        name: String,
        #[arg(
            long,
            help = "CPU cores to allocate (1, 2, 3, or 4; default: 1)",
            value_parser = clap::value_parser!(u32).range(1..=4)
        )]
        cpu: Option<u32>,
        #[arg(
            long,
            short = 'm',
            help = "Memory to allocate (512M, 1G, 2G, or 4G; default: 512M)",
            value_parser = parse_memory_choice
        )]
        memory: Option<u32>,
        #[arg(
            long,
            short = 'H',
            help = "Hypervisor type: firecracker, qemu, or unspecified (default)"
        )]
        hypervisor: Option<String>,
    },
    /// Activate/Rollback to a specific deployment
    Activate {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short = 'd', help = "The Deployment ID to activate")]
        deployment_id: String,
    },
    /// List deployment history for an application
    Deployments {
        #[arg(long, short, help = "Name of the application")]
        name: String,
    },
    /// Stream deployment events for an application
    Watch {
        #[arg(long, short, help = "Name of the application")]
        name: String,
    },
    /// Show the GitHub webhook secret for an application
    Secret {
        #[arg(long, short, help = "Name of the application")]
        name: String,
    },
    /// Configure scaling and autoscaling for an application.
    /// Note: All apps scale to zero automatically after inactivity.
    Scale {
        #[arg(long, short, help = "Name of the application")]
        name: String,
        #[arg(
            long,
            short = 'r',
            help = "Desired fixed replicas (0-3, disables autoscaling if set)"
        )]
        replicas: Option<i32>,
        #[arg(long, help = "Enable/disable autoscaling (--auto true/false)")]
        auto: Option<bool>,
        #[arg(long, short = 'M', help = "Maximum replicas for autoscaling (1-3)")]
        max: Option<i32>,
        #[arg(long, short = 'c', help = "CPU threshold percentage for autoscaling")]
        cpu: Option<f64>,
        #[arg(
            long,
            short = 'e',
            help = "Memory threshold percentage for autoscaling"
        )]
        mem: Option<f64>,
    },
}

#[derive(Subcommand, Debug)]
pub enum DeploymentCommands {
    /// List all active deployments (jobs) across all apps
    List,
    /// Get detailed status of a live deployment (job)
    Status {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID for this instance")]
        job_id: String,
    },
    /// Fetch or tail logs for a deployment
    Logs {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID for this instance")]
        job_id: String,
        #[arg(long, short, help = "Follow the log stream in real-time")]
        follow: bool,
    },
    /// Stop a running deployment (kills the instance)
    Stop {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID for this instance")]
        job_id: String,
    },
    /// Pause a running deployment (suspends CPU)
    Pause {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID for this instance")]
        job_id: String,
    },
    /// Resume a paused deployment
    Resume {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID for this instance")]
        job_id: String,
    },
    /// Remove a deployment record from history
    Delete {
        #[arg(long, short, help = "Name of the application")]
        app: String,
        #[arg(long, short, help = "The unique Job ID to remove")]
        job_id: String,
    },
    /// Stream all cluster-wide deployment events
    Watch,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Display current CLI settings
    Show,
    /// Set a configuration value (e.g., api-url)
    Set {
        #[arg(help = "The configuration key (e.g., api-url)")]
        key: String,
        #[arg(help = "The value to set")]
        value: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SystemCommands {
    /// Check the health of all system services
    Health,
    /// Stream system health updates in real-time
    Watch,
}
