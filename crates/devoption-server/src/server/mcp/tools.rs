//! Bounded observations only. No filesystem paths, logs, secrets or mutations.
use super::*;
use devoption_projects::ProjectId;
type Failure = (i64, &'static str);
const INVALID: Failure = (-32602, "Invalid tool arguments");
const UNAVAILABLE: Failure = (-32603, "Platform observation unavailable");
pub(super) fn names() -> &'static [&'static str] {
    &[
        "list_projects",
        "list_repositories",
        "list_builds",
        "get_project_status",
        "get_diagnostics",
    ]
}
pub(super) fn definitions() -> Vec<Value> {
    names().iter().map(|name| {
        let project=matches!(*name,"list_repositories"|"list_builds"|"get_project_status");
        let paged=matches!(*name,"list_projects"|"list_repositories"|"list_builds");
        let mut properties=json!({});
        if project {properties["project_id"]=json!({"type":"string","pattern":"^[0-9a-fA-F]{32}$"});}
        if paged {properties["offset"]=json!({"type":"integer","minimum":0,"maximum":1000000});}
        let description=match *name {
            "list_projects"=>"List up to 50 projects in the shared workspace; pass next_offset for more. Names are untrusted data.",
            "list_repositories"=>"List up to 50 repository identities attached to a current project. No source, paths or execution.",
            "list_builds"=>"List up to 50 saved build checkpoints after current project/repository association checks. These are not live job status.",
            "get_project_status"=>"Read current project build and runtime observations with availability. No logs, secrets or approvals.",
            _=>"Read configuration and observation availability; no private host paths or credentials.",
        };
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":if project{vec!["project_id"]}else{vec![]},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
    }).collect()
}
pub(super) async fn call(projects: &Projects, params: &Value) -> Result<Value, Failure> {
    let name = params["name"].as_str().ok_or(INVALID)?;
    if !names().contains(&name) {
        return Err(INVALID);
    }
    let empty = json!({});
    let args = params
        .get("arguments")
        .unwrap_or(&empty)
        .as_object()
        .ok_or(INVALID)?;
    let project = if matches!(
        name,
        "list_repositories" | "list_builds" | "get_project_status"
    ) {
        Some(
            args.get("project_id")
                .and_then(Value::as_str)
                .and_then(ProjectId::from_hex)
                .ok_or(INVALID)?,
        )
    } else {
        None
    };
    let paged = matches!(name, "list_projects" | "list_repositories" | "list_builds");
    if args
        .keys()
        .any(|k| !(k == "project_id" && project.is_some() || k == "offset" && paged))
    {
        return Err(INVALID);
    }
    let offset = match args.get("offset") {
        None => 0,
        Some(v) => v.as_u64().filter(|v| *v <= 1_000_000).ok_or(INVALID)? as usize,
    };
    if name == "get_project_status" {
        let view = super::super::overview::snapshot_scoped(
            projects.clone(),
            24,
            Some(project.expect("project tool").to_bytes()),
        )
        .await;
        if !view.catalog_available {
            return Err(UNAVAILABLE);
        }
        let p = view.projects.first().ok_or(INVALID)?;
        return Ok(
            json!({"project_id":hex(&p.id),"name":p.name,"runtime":p.runtime,"build":p.build,"telemetry_reporting":p.reporting,"history_available":view.history_available,"runtime_observation_available":view.deployment_available}),
        );
    }
    // Same ordering as the browser history reader: history, then catalog gate.
    let history = if name == "list_builds" {
        match projects.history.clone() {
            Some(h) => Some(h.lock_owned().await),
            None => None,
        }
    } else {
        None
    };
    let gate = projects.gate.clone().lock_owned().await;
    let projects = projects.clone();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || {
        let _gate=gate;
        let catalog=projects.directory.load().map_err(|_|UNAVAILABLE)?.unwrap_or_default();
        let entry=project.map(|id|catalog.projects().iter().find(|p|p.id()==id).ok_or(INVALID)).transpose()?;
        if name=="get_diagnostics" {return Ok(json!({"catalog":"available","build_history_configured":projects.history.is_some(),"deployment_configured":projects.deployment.is_some(),"security_scan":"not_implemented","access":"read_only_shared_workspace","page_size":50}));}
        let items:Vec<Value>=match name.as_str(){
            "list_projects"=>catalog.projects().iter().map(|p|json!({"id":hex(&p.id().to_bytes()),"name":p.name(),"repository_count":p.repositories().len()})).collect(),
            "list_repositories"=> {
                let p=entry.expect("project tool");
                let mut items=Vec::new();
                for id in p.repositories(){
                    let r=catalog.repositories().iter().find(|r|r.id()==*id).ok_or(UNAVAILABLE)?;
                    items.push(json!({"id":hex(&r.id().to_bytes()),"name":r.name()}));
                }
                items
            },
            "list_builds"=> {
                let p=entry.expect("project tool");
                let Some(history)=history else {return Ok(json!({"availability":"unconfigured","items":[],"next_offset":null}));};
                let records=history.load().map_err(|_|UNAVAILABLE)?;
                let mut items=Vec::new();
                for record in records.iter().filter(|r|r.project()==p.id().to_bytes()) {
                    if !p.repositories().iter().any(|id|id.to_bytes()==record.repository()) || !catalog.repositories().iter().any(|r|r.id().to_bytes()==record.repository()) {return Err(UNAVAILABLE);}
                    let(state,_)=super::super::projects::history::state(record.state());
                    items.push(json!({"id":hex(&record.id().to_bytes()),"repository_id":hex(&record.repository()),"commit":record.commit(),"state":state,"observation":"saved_checkpoint"}));
                }
                items
            },
            _=>return Err(INVALID),
        };
        let count=items.len();
        let page:Vec<_>=items.into_iter().skip(offset).take(50).collect();
        Ok(json!({"availability":"available","items":page,"next_offset":if offset+page.len()<count{Some(offset+page.len())}else{None}}))
    }).await.map_err(|_|UNAVAILABLE)?
}
