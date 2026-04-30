use clap::Subcommand;

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
    /// System Status
    #[command(subcommand)]
    System(SystemCommands),
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
