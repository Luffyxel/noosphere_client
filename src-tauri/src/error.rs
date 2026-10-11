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
    #[error("Cet appareil n’a pas les clés de messagerie de ce compte.")]
    MessagingIdentityUnavailable,
    #[error("Le dépôt Noosnote doit rester privé. Aucune note n’a été publiée.")]
    NoosnoteRepositoryPublic,
    #[error("Ce dépôt ne correspond pas aux notes de ce compte.")]
    NoosnoteRepositoryMismatch,
    #[error("Deux versions de cette note nécessitent un choix. Les deux versions sont conservées.")]
    NoosnoteConflict,
    #[error("Une autre machine vient de synchroniser ses notes. La reprise est automatique.")]
    NoosnoteConcurrentUpdate,
    #[error("Cette page n’a pas encore été chargée. Connecte cette machine pour la récupérer.")]
    NoosnotePageUnavailable,
    #[error("Erreur locale.")]
    Local,
    #[error("La file d’envoi est pleine. Attends la synchronisation avant de renvoyer un message.")]
    OutboxFull,
    #[error("Limite GitHub atteinte. La synchronisation reprendra après le délai demandé.")]
    RateLimited,
    #[error("Une mise à jour est en cours d’installation.")]
    Updating,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Local
    }
}
