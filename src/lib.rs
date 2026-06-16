//! # gitops-operator
//!
//! A pull-mode GitOps controller for Kubernetes. It watches `Deployment` objects
//! annotated with `gitops.operator.*`, and on each reconcile pass it checks the
//! latest commit of an application repository, optionally waits for the matching
//! image to be available in a registry, and then patches and pushes the image tag
//! in a separate manifests repository. A CD tool (e.g. Argo CD) rolls out the
//! change because git is the source of truth.
//!
//! See the project README for the full annotation reference and HTTP API.
//!
//! ## Modules
//!
//! - [`configuration`]: deployment annotation parsing ([`configuration::Entry`]),
//!   the reconcile engine ([`configuration::DeploymentProcessor`]), and the
//!   structured per-deployment result ([`configuration::ReconcileResult`]).
//! - [`files`]: reading and patching the image tag in deployment manifests.
//! - [`git`]: cloning, updating, committing, and pushing repositories over SSH.
//! - [`github`]: querying GitHub Actions build status for a commit.
//! - [`registry`]: checking whether an image tag exists in a container registry.
//! - [`secrets`]: fetching SSH keys, registry, notification, and token secrets.
//! - [`notifications`]: sending Slack-compatible webhook notifications.
//! - [`telemetry`]: tracing, metrics, and OpenTelemetry export setup.
//! - [`traits`]: the dependency-injection interfaces used to test the above.

pub mod configuration;
pub mod files;
pub mod git;
pub mod github;
pub mod notifications;
pub mod registry;
pub mod secrets;
pub mod telemetry;
pub mod traits;
