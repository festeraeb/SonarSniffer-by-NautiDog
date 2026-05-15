pub mod types;
pub mod search_theory;
pub mod dispatch;
pub mod mapping;
pub mod tracking;
pub mod reporting;
pub mod admin;
pub mod plugins;
pub mod agent;
pub mod detection;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::types::*;
use crate::tracking::GpsPoint;

#[derive(Clone)]
pub struct AppState {
    pub cases: Arc<Mutex<Vec<SarCase>>>,
    pub segments: Arc<Mutex<Vec<SearchSegment>>>,
    pub members: Arc<Mutex<Vec<TeamMember>>>,
    pub positions: Arc<Mutex<HashMap<Uuid, Vec<GpsPoint>>>>,
}
