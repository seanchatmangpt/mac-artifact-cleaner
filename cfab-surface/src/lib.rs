//! Cfab Surface & Fabric Graph Library.
//!
//! This crate provides `Surface` and `Fabric` structures for managing networks of connected
//! surfaces using graph algorithms and serialized URLs.

use std::collections::HashMap;

use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
    Direction,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// Errors that can occur during Fabric graph operations.
///
/// # Examples
///
/// ```
/// use cfab_surface::FabricError;
///
/// let err = FabricError::CycleDetected;
/// assert_eq!(err.to_string(), "Cycle detected in the Fabric graph");
/// ```
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum FabricError {
    /// The specified surface could not be found in the fabric.
    #[error("Surface with ID '{id}' not found in the fabric")]
    SurfaceNotFound {
        /// The missing surface identifier.
        id: String,
    },

    /// The connection between surfaces already exists or is invalid.
    #[error("Invalid connection from '{from}' to '{to}': {reason}")]
    InvalidConnection {
        /// Starting surface identifier.
        from: String,
        /// Destination surface identifier.
        to: String,
        /// The reason the connection is invalid.
        reason: String,
    },

    /// There is no path between the specified surfaces.
    #[error("No path exists from '{from}' to '{to}'")]
    NoPathExists {
        /// Starting surface identifier.
        from: String,
        /// Destination surface identifier.
        to: String,
    },

    /// The URL provided is invalid or uses an unsupported scheme.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// A cycle was detected in the graph when adding a dependency or connection.
    #[error("Cycle detected in the Fabric graph")]
    CycleDetected,

    /// An error occurred during state observation.
    #[error("Observation failed: {0}")]
    ObservationError(String),
}

/// Represents the classification variant of the `Surface` digital resource.
///
/// # Examples
///
/// ```
/// use cfab_surface::SurfaceKind;
///
/// let kind = SurfaceKind::LocalDirectory;
/// assert_eq!(kind.scheme(), "file");
/// ```
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    /// Represents a local directory resource (scheme: `file`).
    LocalDirectory,
    /// Represents a remote GitHub repository (scheme: `github`).
    GitHubRepository,
    /// Represents a cleanup/action plan (scheme: `plan`).
    Plan,
    /// Represents an execution receipt (scheme: `receipt`).
    Receipt,
    /// Represents a document or markdown log (scheme: `doc`).
    Document,
}

impl SurfaceKind {
    /// Returns the corresponding `SurfaceKind` based on URL scheme.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::SurfaceKind;
    ///
    /// assert_eq!(SurfaceKind::from_url_scheme("file"), Some(SurfaceKind::LocalDirectory));
    /// assert_eq!(SurfaceKind::from_url_scheme("invalid"), None);
    /// ```
    pub fn from_url_scheme(scheme: &str) -> Option<Self> {
        match scheme {
            "file" => Some(Self::LocalDirectory),
            "github" => Some(Self::GitHubRepository),
            "plan" => Some(Self::Plan),
            "receipt" => Some(Self::Receipt),
            "doc" => Some(Self::Document),
            _ => None,
        }
    }

    /// Returns the canonical scheme string for this variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::SurfaceKind;
    ///
    /// assert_eq!(SurfaceKind::LocalDirectory.scheme(), "file");
    /// assert_eq!(SurfaceKind::GitHubRepository.scheme(), "github");
    /// ```
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::LocalDirectory => "file",
            Self::GitHubRepository => "github",
            Self::Plan => "plan",
            Self::Receipt => "receipt",
            Self::Document => "doc",
        }
    }
}

/// Represents a distinct node or resource node within the `Fabric`.
///
/// # Examples
///
/// ```
/// use cfab_surface::Surface;
/// use url::Url;
///
/// let url = Url::parse("file:///path/to/local").unwrap();
/// let surface = Surface::new("s1".to_string(), url, "S1".to_string());
/// assert_eq!(surface.id, "s1");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Surface {
    /// Unique identifier for the surface.
    pub id: String,
    /// The canonical URL associated with the surface.
    pub url: Url,
    /// User-friendly label or name for the surface.
    pub name: String,
    /// The variant category of the surface resource.
    pub kind: SurfaceKind,
}

/// The observed snapshot state of a `Surface`.
///
/// # Examples
///
/// ```
/// use cfab_surface::SurfaceState;
/// use std::collections::HashMap;
///
/// let state = SurfaceState {
///     exists: true,
///     metadata: HashMap::new(),
/// };
/// assert!(state.exists);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceState {
    /// Indicates whether the physical or logical resource exists.
    pub exists: bool,
    /// Key-value metadata captured during observation.
    pub metadata: HashMap<String, String>,
}

impl Surface {
    /// Creates a new `Surface` instance with scheme auto-deduction.
    ///
    /// Defaults to `LocalDirectory` if the scheme is not recognized.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Surface;
    /// use url::Url;
    ///
    /// let url = Url::parse("file:///path/to/local").unwrap();
    /// let surface = Surface::new("s1".to_string(), url, "S1".to_string());
    /// assert_eq!(surface.id, "s1");
    /// ```
    pub fn new(id: String, url: Url, name: String) -> Self {
        let kind =
            SurfaceKind::from_url_scheme(url.scheme()).unwrap_or(SurfaceKind::LocalDirectory);
        Self { id, url, name, kind }
    }

    /// Parses and validates a `Surface` with strict scheme and path verification.
    ///
    /// # Errors
    ///
    /// Returns `FabricError::InvalidUrl` if the URL is unsupported or malformed.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Surface;
    /// use url::Url;
    ///
    /// let url = Url::parse("github://owner/repo").unwrap();
    /// let surface = Surface::from_url("gh1".to_string(), url, "GH Repo".to_string()).unwrap();
    /// assert_eq!(surface.kind, cfab_surface::SurfaceKind::GitHubRepository);
    ///
    /// let bad_url = Url::parse("http://example.com").unwrap();
    /// assert!(Surface::from_url("bad".to_string(), bad_url, "Bad".to_string()).is_err());
    /// ```
    pub fn from_url(id: String, url: Url, name: String) -> Result<Self, FabricError> {
        let kind = SurfaceKind::from_url_scheme(url.scheme()).ok_or_else(|| {
            FabricError::InvalidUrl(format!("Unsupported scheme: {}", url.scheme()))
        })?;

        // Specific structural validation rules
        match kind {
            SurfaceKind::GitHubRepository => {
                if url.host_str().is_none() {
                    return Err(FabricError::InvalidUrl(
                        "github URI must specify an owner (host)".into(),
                    ));
                }
                let repo = url.path().trim_start_matches('/');
                if repo.is_empty() {
                    return Err(FabricError::InvalidUrl(
                        "github URI must specify a repository (path)".into(),
                    ));
                }
            }
            _ => {
                if url.path().trim_end_matches('/').is_empty() {
                    return Err(FabricError::InvalidUrl(format!(
                        "{} URI must have a non-empty path",
                        url.scheme()
                    )));
                }
            }
        }

        Ok(Self { id, url, name, kind })
    }

    /// Observes the current state ($O^*$) and metadata of the resource.
    ///
    /// For local filesystem resources (`file`, `plan`, `receipt`, `doc`), this checks presence
    /// and extracts size and modification times. For remote/logical resources (`github`), it parses
    /// URLs offline.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Surface;
    /// use url::Url;
    ///
    /// let url = Url::parse("github://owner/repo").unwrap();
    /// let surface = Surface::from_url("gh1".to_string(), url, "GH Repo".to_string()).unwrap();
    /// let state = surface.observe_state().unwrap();
    /// assert!(state.exists);
    /// assert_eq!(state.metadata.get("owner").unwrap(), "owner");
    /// ```
    pub fn observe_state(&self) -> Result<SurfaceState, FabricError> {
        let mut metadata = HashMap::new();
        match self.kind {
            SurfaceKind::LocalDirectory
            | SurfaceKind::Plan
            | SurfaceKind::Receipt
            | SurfaceKind::Document => {
                let mut file_url = self.url.clone();
                if file_url.scheme() != "file" {
                    let _ = file_url.set_scheme("file");
                }
                let path = file_url.to_file_path().map_err(|_| {
                    FabricError::ObservationError(format!(
                        "Failed to convert URL to file path: {}",
                        self.url
                    ))
                })?;
                let exists = path.exists();
                metadata.insert("path".to_string(), path.to_string_lossy().to_string());
                if exists {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        metadata.insert("size_bytes".to_string(), meta.len().to_string());
                        metadata.insert("is_dir".to_string(), meta.is_dir().to_string());
                        metadata.insert("is_file".to_string(), meta.is_file().to_string());
                        if let Ok(modified) = meta.modified() {
                            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                                metadata.insert(
                                    "modified_timestamp".to_string(),
                                    duration.as_secs().to_string(),
                                );
                            }
                        }
                    }
                }
                Ok(SurfaceState { exists, metadata })
            }
            SurfaceKind::GitHubRepository => {
                let owner = self.url.host_str().ok_or_else(|| {
                    FabricError::InvalidUrl("GitHub URL must contain an owner".to_string())
                })?;
                let repo = self.url.path().trim_start_matches('/');
                metadata.insert("owner".to_string(), owner.to_string());
                metadata.insert("repo".to_string(), repo.to_string());

                // GitHub repositories are considered logically valid in the network representation.
                Ok(SurfaceState { exists: true, metadata })
            }
        }
    }
}

/// The specific relation category for directed edges in the `Fabric` graph.
///
/// # Examples
///
/// ```
/// use cfab_surface::RelationKind;
///
/// let kind = RelationKind::Dependency;
/// assert_eq!(kind, RelationKind::Dependency);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelationKind {
    /// Represents dependency or parent-child relations.
    Dependency,
    /// Represents transformation mapping ($\mu$) from one surface to another.
    Transformation {
        /// Identifier of the mapping function/logic.
        mapping_id: String,
    },
    /// Represents an evidence turnstile relationship ($R \vdash A = \mu(O^*)$).
    Evidence {
        /// Identifier of the evaluator or turnstile checker.
        evaluator_id: String,
    },
}

/// Represents the directed, semantic edge between surfaces.
///
/// # Examples
///
/// ```
/// use cfab_surface::{Relation, RelationKind};
///
/// let rel = Relation::new(RelationKind::Dependency, 1.5);
/// assert_eq!(rel.weight, 1.5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relation {
    /// The relation variant category.
    pub kind: RelationKind,
    /// Numerical weight/cost for pathfinding algorithms.
    pub weight: f64,
}

impl Relation {
    /// Constructor for a new `Relation`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::{Relation, RelationKind};
    ///
    /// let rel = Relation::new(RelationKind::Dependency, 1.0);
    /// assert_eq!(rel.weight, 1.0);
    /// ```
    pub fn new(kind: RelationKind, weight: f64) -> Self {
        assert!(weight.is_finite(), "Relation weight must be finite (not NaN or Infinity)");
        Self { kind, weight }
    }
}

/// Represents a directed network of connected `Surface` instances.
///
/// # Examples
///
/// ```
/// use cfab_surface::Fabric;
///
/// let fabric = Fabric::new();
/// assert!(fabric.is_empty());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fabric {
    /// Map of surface IDs to their corresponding graph node indices.
    node_map: HashMap<String, NodeIndex>,
    /// Map of node indices back to the Surface data.
    ///
    /// We map indices to `usize` for clean, error-free JSON serialization.
    surfaces: HashMap<usize, Surface>,
    /// The directed graph representing connections between surfaces.
    graph: DiGraph<String, Relation>,
}

impl Default for Fabric {
    fn default() -> Self {
        Self::new()
    }
}

impl Fabric {
    /// Creates a new, empty `Fabric`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Fabric;
    ///
    /// let fabric = Fabric::new();
    /// assert_eq!(fabric.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self { node_map: HashMap::new(), surfaces: HashMap::new(), graph: DiGraph::new() }
    }

    /// Returns the number of surfaces in the fabric.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Fabric;
    ///
    /// let fabric = Fabric::new();
    /// assert_eq!(fabric.len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns true if the fabric contains no surfaces.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::Fabric;
    ///
    /// let fabric = Fabric::new();
    /// assert!(fabric.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Adds a new `Surface` to the fabric.
    ///
    /// If a surface with the same ID already exists, it is overwritten and its index returned,
    /// provided the update does not violate edge validation rules for any incoming or outgoing edges.
    ///
    /// # Errors
    ///
    /// Returns `FabricError::InvalidConnection` if the update would violate edge rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::{Fabric, Surface};
    /// use url::Url;
    ///
    /// let mut fabric = Fabric::new();
    /// let url = Url::parse("file:///path/to/s1").unwrap();
    /// let surface = Surface::new("s1".to_string(), url, "S1".to_string());
    ///
    /// fabric.add_surface(surface).unwrap();
    /// assert_eq!(fabric.len(), 1);
    /// ```
    pub fn add_surface(&mut self, surface: Surface) -> Result<NodeIndex, FabricError> {
        if let Some(&index) = self.node_map.get(&surface.id) {
            let new_kind = surface.kind;

            // Check outgoing edges
            for edge in self.graph.edges_directed(index, Direction::Outgoing) {
                let target_idx = edge.target();
                let target_surface = self.surfaces.get(&target_idx.index()).ok_or_else(|| {
                    FabricError::SurfaceNotFound { id: self.graph[target_idx].clone() }
                })?;
                Self::validate_edge_rule(&new_kind, &target_surface.kind, &edge.weight().kind)
                    .map_err(|reason| FabricError::InvalidConnection {
                        from: surface.id.clone(),
                        to: target_surface.id.clone(),
                        reason,
                    })?;
            }

            // Check incoming edges
            for edge in self.graph.edges_directed(index, Direction::Incoming) {
                let source_idx = edge.source();
                let source_surface = self.surfaces.get(&source_idx.index()).ok_or_else(|| {
                    FabricError::SurfaceNotFound { id: self.graph[source_idx].clone() }
                })?;
                Self::validate_edge_rule(&source_surface.kind, &new_kind, &edge.weight().kind)
                    .map_err(|reason| FabricError::InvalidConnection {
                        from: source_surface.id.clone(),
                        to: surface.id.clone(),
                        reason,
                    })?;
            }

            self.surfaces.insert(index.index(), surface);
            Ok(index)
        } else {
            let id = surface.id.clone();
            let index = self.graph.add_node(id.clone());
            self.node_map.insert(id, index);
            self.surfaces.insert(index.index(), surface);
            Ok(index)
        }
    }

    /// Validates a potential edge connection against domain directional logic.
    ///
    /// # Edge Validation Rules
    ///
    /// - A `Receipt` cannot point to a `Plan` (must be `Plan` -> `Receipt`).
    /// - A `Receipt` cannot point to a `LocalDirectory` or `GitHubRepository`.
    /// - A `Plan` cannot point to a `GitHubRepository`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::{Fabric, SurfaceKind, RelationKind};
    ///
    /// assert!(Fabric::validate_edge_rule(&SurfaceKind::Plan, &SurfaceKind::Receipt, &RelationKind::Dependency).is_ok());
    /// assert!(Fabric::validate_edge_rule(&SurfaceKind::Receipt, &SurfaceKind::Plan, &RelationKind::Dependency).is_err());
    /// ```
    pub fn validate_edge_rule(
        from_kind: &SurfaceKind,
        to_kind: &SurfaceKind,
        _relation_kind: &RelationKind,
    ) -> Result<(), String> {
        match (from_kind, to_kind) {
            (SurfaceKind::Receipt, SurfaceKind::Plan) => {
                Err("A Receipt cannot point to a Plan".to_string())
            }
            (SurfaceKind::Receipt, SurfaceKind::LocalDirectory) => {
                Err("A Receipt cannot point to a LocalDirectory".to_string())
            }
            (SurfaceKind::Receipt, SurfaceKind::GitHubRepository) => {
                Err("A Receipt cannot point to a GitHubRepository".to_string())
            }
            (SurfaceKind::Plan, SurfaceKind::GitHubRepository) => {
                Err("A Plan cannot point to a GitHubRepository".to_string())
            }
            _ => Ok(()),
        }
    }

    /// Connects two surfaces with a directed, validated edge.
    ///
    /// Ensures that the edge does not violate edge direction rules and does not introduce cycles.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - `FabricError::SurfaceNotFound` if either surface does not exist.
    /// - `FabricError::InvalidConnection` if connection violates edge rules, weight is non-finite, or connection already exists.
    /// - `FabricError::CycleDetected` if adding the edge creates a cyclic dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::{Fabric, Surface, Relation, RelationKind};
    /// use url::Url;
    ///
    /// let mut fabric = Fabric::new();
    /// let s1 = Surface::new("s1".to_string(), Url::parse("plan:///p1.json").unwrap(), "Plan".to_string());
    /// let s2 = Surface::new("s2".to_string(), Url::parse("receipt:///r1.json").unwrap(), "Receipt".to_string());
    ///
    /// fabric.add_surface(s1).unwrap();
    /// fabric.add_surface(s2).unwrap();
    ///
    /// let rel = Relation::new(RelationKind::Dependency, 1.0);
    ///
    /// // Valid edge
    /// assert!(fabric.connect("s1", "s2", rel.clone()).is_ok());
    ///
    /// // Invalid edge (Receipt pointing to Plan)
    /// assert!(fabric.connect("s2", "s1", rel).is_err());
    /// ```
    pub fn connect(&mut self, from: &str, to: &str, relation: Relation) -> Result<(), FabricError> {
        let from_idx = *self
            .node_map
            .get(from)
            .ok_or_else(|| FabricError::SurfaceNotFound { id: from.to_string() })?;
        let to_idx = *self
            .node_map
            .get(to)
            .ok_or_else(|| FabricError::SurfaceNotFound { id: to.to_string() })?;

        if !relation.weight.is_finite() {
            return Err(FabricError::InvalidConnection {
                from: from.to_string(),
                to: to.to_string(),
                reason: "Relation weight must be finite (not NaN or Infinity)".to_string(),
            });
        }

        if self.graph.find_edge(from_idx, to_idx).is_some() {
            return Err(FabricError::InvalidConnection {
                from: from.to_string(),
                to: to.to_string(),
                reason: "A connection between these surfaces already exists".to_string(),
            });
        }

        // Validate edge rules
        let from_surface = self
            .surfaces
            .get(&from_idx.index())
            .ok_or_else(|| FabricError::SurfaceNotFound { id: from.to_string() })?;
        let to_surface = self
            .surfaces
            .get(&to_idx.index())
            .ok_or_else(|| FabricError::SurfaceNotFound { id: to.to_string() })?;

        Self::validate_edge_rule(&from_surface.kind, &to_surface.kind, &relation.kind).map_err(
            |reason| FabricError::InvalidConnection {
                from: from.to_string(),
                to: to.to_string(),
                reason,
            },
        )?;

        // Temporarily insert edge
        let edge_idx = self.graph.add_edge(from_idx, to_idx, relation);

        // Check for cycles
        if petgraph::algo::is_cyclic_directed(&self.graph) {
            self.graph.remove_edge(edge_idx);
            return Err(FabricError::CycleDetected);
        }

        Ok(())
    }

    /// Finds the shortest path from one surface to another.
    ///
    /// # Errors
    ///
    /// Returns `FabricError::SurfaceNotFound` if either surface does not exist,
    /// or `FabricError::NoPathExists` if no route can be found.
    ///
    /// # Examples
    ///
    /// ```
    /// use cfab_surface::{Fabric, Surface, Relation, RelationKind};
    /// use url::Url;
    ///
    /// let mut fabric = Fabric::new();
    /// let s1 = Surface::new("s1".to_string(), Url::parse("file:///s1").unwrap(), "S1".to_string());
    /// let s2 = Surface::new("s2".to_string(), Url::parse("plan:///p2.json").unwrap(), "S2".to_string());
    /// let s3 = Surface::new("s3".to_string(), Url::parse("receipt:///r3.json").unwrap(), "S3".to_string());
    ///
    /// fabric.add_surface(s1).unwrap();
    /// fabric.add_surface(s2).unwrap();
    /// fabric.add_surface(s3).unwrap();
    ///
    /// fabric.connect("s1", "s2", Relation::new(RelationKind::Dependency, 1.0)).unwrap();
    /// fabric.connect("s2", "s3", Relation::new(RelationKind::Dependency, 1.0)).unwrap();
    ///
    /// let path = fabric.find_path("s1", "s3").unwrap();
    /// assert_eq!(path.len(), 3);
    /// assert_eq!(path[0].id, "s1");
    /// assert_eq!(path[2].id, "s3");
    /// ```
    pub fn find_path(&self, from: &str, to: &str) -> Result<Vec<Surface>, FabricError> {
        let from_idx = self
            .node_map
            .get(from)
            .ok_or_else(|| FabricError::SurfaceNotFound { id: from.to_string() })?;
        let to_idx = self
            .node_map
            .get(to)
            .ok_or_else(|| FabricError::SurfaceNotFound { id: to.to_string() })?;

        let path_indices = petgraph::algo::astar(
            &self.graph,
            *from_idx,
            |finish| finish == *to_idx,
            |e| e.weight().weight,
            |_| 0.0,
        )
        .ok_or_else(|| FabricError::NoPathExists { from: from.to_string(), to: to.to_string() })?;

        let mut path_surfaces = Vec::new();
        for node_idx in path_indices.1 {
            if let Some(surface) = self.surfaces.get(&node_idx.index()) {
                path_surfaces.push(surface.clone());
            }
        }

        Ok(path_surfaces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_creation_and_validation() {
        let url = Url::parse("file:///Users/sac/osx-clnr/src").unwrap();
        let s = Surface::from_url("s1".to_string(), url, "Source".to_string()).unwrap();
        assert_eq!(s.kind, SurfaceKind::LocalDirectory);

        let url = Url::parse("github://seanchatmangpt/mac-artifact-cleaner").unwrap();
        let s = Surface::from_url("s2".to_string(), url, "GH Repo".to_string()).unwrap();
        assert_eq!(s.kind, SurfaceKind::GitHubRepository);

        let url = Url::parse("plan:///Users/sac/osx-clnr/plan.json").unwrap();
        let s = Surface::from_url("s3".to_string(), url, "Plan".to_string()).unwrap();
        assert_eq!(s.kind, SurfaceKind::Plan);

        let url = Url::parse("receipt:///Users/sac/osx-clnr/receipt.json").unwrap();
        let s = Surface::from_url("s4".to_string(), url, "Receipt".to_string()).unwrap();
        assert_eq!(s.kind, SurfaceKind::Receipt);

        let url = Url::parse("doc:///Users/sac/osx-clnr/README.md").unwrap();
        let s = Surface::from_url("s5".to_string(), url, "Readme".to_string()).unwrap();
        assert_eq!(s.kind, SurfaceKind::Document);
    }

    #[test]
    fn test_surface_validation_failures() {
        // Missing owner/repo in github URI
        let url = Url::parse("github:///only-path").unwrap();
        assert!(Surface::from_url("bad1".to_string(), url, "Bad".to_string()).is_err());

        // Unsupported scheme
        let url = Url::parse("https://github.com/owner/repo").unwrap();
        assert!(Surface::from_url("bad2".to_string(), url, "Bad".to_string()).is_err());

        // Empty path in file URI
        let url = Url::parse("file://").unwrap();
        assert!(Surface::from_url("bad3".to_string(), url, "Bad".to_string()).is_err());
    }

    #[test]
    fn test_edge_rule_validation() {
        let mut fabric = Fabric::new();
        let plan = Surface::new(
            "p1".to_string(),
            Url::parse("plan:///p1.json").unwrap(),
            "Plan".to_string(),
        );
        let receipt = Surface::new(
            "r1".to_string(),
            Url::parse("receipt:///r1.json").unwrap(),
            "Receipt".to_string(),
        );

        fabric.add_surface(plan).unwrap();
        fabric.add_surface(receipt).unwrap();

        let rel = Relation::new(RelationKind::Dependency, 1.0);

        // Valid Plan -> Receipt
        assert!(fabric.connect("p1", "r1", rel.clone()).is_ok());

        // Invalid Receipt -> Plan
        let mut fabric2 = Fabric::new();
        let plan = Surface::new(
            "p1".to_string(),
            Url::parse("plan:///p1.json").unwrap(),
            "Plan".to_string(),
        );
        let receipt = Surface::new(
            "r1".to_string(),
            Url::parse("receipt:///r1.json").unwrap(),
            "Receipt".to_string(),
        );
        fabric2.add_surface(plan).unwrap();
        fabric2.add_surface(receipt).unwrap();
        assert!(fabric2.connect("r1", "p1", rel).is_err());
    }

    #[test]
    fn test_cycle_detection() {
        let mut fabric = Fabric::new();
        let s1 =
            Surface::new("s1".to_string(), Url::parse("file:///s1").unwrap(), "S1".to_string());
        let s2 =
            Surface::new("s2".to_string(), Url::parse("file:///s2").unwrap(), "S2".to_string());
        let s3 =
            Surface::new("s3".to_string(), Url::parse("file:///s3").unwrap(), "S3".to_string());

        fabric.add_surface(s1).unwrap();
        fabric.add_surface(s2).unwrap();
        fabric.add_surface(s3).unwrap();

        let rel = Relation::new(RelationKind::Dependency, 1.0);
        fabric.connect("s1", "s2", rel.clone()).unwrap();
        fabric.connect("s2", "s3", rel.clone()).unwrap();

        // Adding s3 -> s1 would form a cycle
        let result = fabric.connect("s3", "s1", rel);
        assert!(matches!(result, Err(FabricError::CycleDetected)));

        // Verify path still exists and graph remains acyclic
        let path = fabric.find_path("s1", "s3").unwrap();
        assert_eq!(path.len(), 3);
    }
}
