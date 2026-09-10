use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Données invalides.")]
    InvalidData,
    #[error("Réponse GitHub invalide.")]
    InvalidGitHubResponse,
    #[error("Ressource GitHub trop volumineuse.")]
    ResponseTooLarge,
    #[error("Session GitHub expirée.")]
    SessionExpired,
    #[error("GitHub a refusé la requête.")]
    GitHubDenied,
    #[error("Ressource GitHub introuvable.")]
    GitHubNotFound,
    #[error("Conflit avec une ressource GitHub existante.")]
    GitHubConflict,
    #[error("Autorisation GitHub refusée.")]
    AuthorizationDenied,
    #[error("Le code GitHub a expiré.")]
    AuthorizationExpired,
    #[error("Configuration GitHub déjà en cours.")]
    SetupInProgress,
    #[error("Installation GitHub App invalide.")]
    InvalidInstallation,
    #[error("Stockage sécurisé indisponible.")]
    SecureStorageUnavailable,
    #[error("Trop d’instances Noosphere sont déjà ouvertes.")]
    TooManyInstances,
    #[error("Ce compte Noosphere est déjà ouvert dans une autre fenêtre.")]
    AccountAlreadyOpen,
    #[error("Impossible de contacter GitHub.")]
    Network,
    #[error("Données cryptographiques invalides.")]
    Crypto,
    #[error("Erreur locale.")]
    Local,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Local
    }
}
