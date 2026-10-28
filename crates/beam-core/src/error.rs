use thiserror::Error;

#[derive(Error, Debug)]
pub enum BeamError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("RAW decode error: {0}")]
    RawDecode(String),

    #[error("Image processing error: {0}")]
    ImageProcessing(String),

    #[error("Unsupported image format: {0}")]
    UnsupportedFormat(String),

    #[error("Sidecar serialization error: {0}")]
    SidecarSerialization(#[from] ron::Error),

    #[error("Sidecar deserialization error: {0}")]
    SidecarDeserialization(#[from] ron::error::SpannedError),

    #[error("Sidecar migration error: {0}")]
    SidecarMigration(String),

    #[error("GPU pipeline error: {0}")]
    GpuPipeline(String),

    #[error("Library database error: {0}")]
    Database(String),
}

pub type Result<T> = std::result::Result<T, BeamError>;
