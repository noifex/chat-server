pub type BoardResult<T> = Result<T, BoardError>;

#[derive(Debug)]
pub enum BoardError {
    Usage(String),
    Rejected(String),
    Lock(std::io::Error),
    WalPoison(String),
    WalInDoubt(std::io::Error),
    Git(String),
    //RetryLater(String),
    Invariant(String),
    Json(serde_json::Error),
    Time(std::time::SystemTimeError),
    Exhausted(&'static str),
}

impl BoardError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Rejected(_) => 1,

            Self::Lock(_) => 74,
            Self::WalPoison(_) => 75,
            Self::WalInDoubt(_) => 75,
            Self::Git(_) => 75,
            //Self::RetryLater(_)=>75,
            Self::Invariant(_) => 37,

            Self::Json(_) => 70,
            Self::Time(_) => 70,
            Self::Exhausted(_) => 70,
        }
    }
}
impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "usage:{message}"),
            Self::Rejected(message) => write!(f, "rejected:{message}"),
            Self::Lock(message) => write!(f, "lock:{message}"),
            Self::WalPoison(message) => write!(f, "poison:{message}"),
            Self::WalInDoubt(message) => write!(f, "indoubt:{message}"),
            Self::Git(message) => write!(f, "git:{message}"),
            //Self::RetryLater(message)=>write!(f,"retry later:{message}"),
            Self::Invariant(message) => write!(f, "invariant:{message}"),
            Self::Json(message) => write!(f, "json:{message}"),
            Self::Time(message) => write!(f, "time:{message}"),
            Self::Exhausted(message) => write!(f, "exhausted:{message}"),
        }
    }
}
impl std::error::Error for BoardError {}
