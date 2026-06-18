//! Background orchestration services.
//!
//! Declared empty in Task 1. Later waves add the non-protocol orchestration that keeps
//! the LSP event loop responsive: analysis scheduling, debounced re-analysis, and
//! sidecar lifecycle management.
//!
//! TODO(later waves): move heavy, non-UI work (parsing, semantic extraction, sidecar
//! calls) onto worker tasks here so protocol handlers stay non-blocking.
