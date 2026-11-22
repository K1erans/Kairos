//! A notification at a task's due time. Linux has no store of scheduled notifications, so the
//! app keeps the plan and fires each one from its timer as its time comes.

use crate::id::Id;
use crate::models::{AppData, TaskItem, TaskStatus};
use crate::time::Date;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DueReminder {
    pub id: String,
    pub title: String,
    pub body: String,
    pub fire_at: Date,
}

pub const ID_PREFIX: &str = "kairos-due-";
pub const LIMIT: usize = 64;

/// What the reminders depend on, compared before planning again.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    tasks: Vec<TaskItem>,
    group_names: BTreeMap<Id, String>,
    on: bool,
}

impl Inputs {
    pub fn new(data: &AppData) -> Self {
        let mut names = BTreeMap::new();
        for g in &data.groups {
            names.entry(g.id).or_insert_with(|| g.name.clone());
        }
        Inputs {
            tasks: data.tasks.iter().filter(|t| t.status == TaskStatus::Todo && t.due.is_some()).cloned().collect(),
            group_names: names,
            on: data.settings.notifies_when_due,
        }
    }
}

/// The soonest unfinished tasks due after `now`, at most `LIMIT` of them.
pub fn plan(inputs: &Inputs, now: Date) -> Vec<DueReminder> {
    if !inputs.on {
        return vec![];
    }
    let mut upcoming: Vec<(&TaskItem, Date)> = inputs.tasks.iter().filter_map(|t| t.due.filter(|d| *d > now).map(|d| (t, d))).collect();
    upcoming.sort_by(|a, b| (a.1, a.0.id.string()).cmp(&(b.1, b.0.id.string())));
    upcoming
        .into_iter()
        .take(LIMIT)
        .map(|(t, due)| {
            let group = t.group_id.and_then(|g| inputs.group_names.get(&g));
            DueReminder {
                id: format!("{ID_PREFIX}{}", t.id.string()),
                title: t.title.clone(),
                body: group.map(|g| format!("Due now, in {g}.")).unwrap_or_else(|| "Due now.".into()),
                fire_at: due,
            }
        })
        .collect()
}
