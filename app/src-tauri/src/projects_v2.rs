// GitHub Projects V2（GraphQL）只读访问层：项目列表 / project 字段与 items、scope 检测、内存缓存
//
// 认证结论（issue #29）：只能用 classic PAT 勾选 project scope；老 token 没有该 scope，
// GitHub 返回 200 + INSUFFICIENT_SCOPES errors，映射为 ERR_MISSING_PROJECT_SCOPE，
// 前端按此前缀识别并引导重新生成 token；HTTP 401（Bad credentials，token 失效/被吊销）
// 单独映射为 ERR_BAD_CREDENTIALS。

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::github::{ensure_token, Http};
use crate::AppState;

/// 前端按此字符串前缀识别 typed error，引导用户重新授权
pub const ERR_MISSING_PROJECT_SCOPE: &str =
    "MISSING_PROJECT_SCOPE: 当前 token 无权访问 Projects V2，请重新生成 classic PAT 并勾选 project scope 后重新登录";

/// HTTP 401 = token 无效或已被吊销（Bad credentials），与缺 scope 是两种情形
pub const ERR_BAD_CREDENTIALS: &str = "BAD_CREDENTIALS: GitHub token 无效或已被吊销，请重新登录";

/// items 列表缓存有效期；refresh=true 可强制绕过
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(300);
/// items 翻页上限（每页 100 条），控制 GraphQL cost
const MAX_ITEM_PAGES: usize = 5;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectV2Info {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub short_description: String,
    pub url: String,
    pub closed: bool,
    pub updated_at: String,
    pub owner_login: String,
    pub owner_type: String, // "user" | "org"
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectV2FieldOption {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectV2Field {
    pub id: String,
    pub name: String,
    pub data_type: String, // "SINGLE_SELECT" / "TEXT" / "NUMBER" / "DATE" / "ITERATION" ...
    /// 仅单选字段（如 Status）有值
    pub options: Vec<ProjectV2FieldOption>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectV2Item {
    pub id: String,
    pub content_type: String, // "Issue" | "PullRequest" | "DraftIssue" | ""（已删除的内容）
    pub title: String,
    pub number: Option<u64>,
    pub state: String,
    pub url: String,
    pub repo: String, // nameWithOwner
    /// Status 单选字段当前值（无 Status 字段或未设置时为 None）
    pub status: Option<String>,
    pub status_option_id: Option<String>,
    pub updated_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectV2Board {
    pub project: ProjectV2Info,
    pub fields: Vec<ProjectV2Field>,
    pub items: Vec<ProjectV2Item>,
    /// project 内 items 总数（GraphQL totalCount）
    pub total_count: u64,
    /// 达到翻页上限（MAX_ITEM_PAGES × 100 条）未拉全时为 true
    pub truncated: bool,
}

/// get_project_v2 的内存缓存（挂在 AppState）
pub struct BoardCache {
    pub key: String, // `${token指纹}:${project_id}`
    pub board: ProjectV2Board,
    pub fetched_at: std::time::Instant,
}

fn token_fp(token: &str) -> &str {
    &token[token.len().saturating_sub(6)..]
}

/// GraphQL POST；HTTP 401 映射为 ERR_BAD_CREDENTIALS，响应里的 INSUFFICIENT_SCOPES 映射为 ERR_MISSING_PROJECT_SCOPE
async fn gql(http: &Http, token: &str, query: &str, variables: Value) -> Result<Value, String> {
    let resp = http
        .send(|c| {
            c.post("https://api.github.com/graphql")
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .json(&json!({"query": query, "variables": variables}))
        })
        .await?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if status.as_u16() == 401 {
        return Err(ERR_BAD_CREDENTIALS.into());
    }
    if !status.is_success() {
        return Err(format!("GitHub GraphQL {status}: {}", &text[..text.len().min(300)]));
    }
    serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}"))
}

/// 已知 token 缺 project scope 时 fail-fast，省一次注定失败的 GraphQL 调用
fn check_known_scope(state: &AppState) -> Result<(), String> {
    if *state.has_project_scope.lock().unwrap() == Some(false) {
        return Err(ERR_MISSING_PROJECT_SCOPE.into());
    }
    Ok(())
}

/// 响应 errors 中只要出现 scope 不足就映射为 typed error
fn check_scope_errors(v: &Value) -> Result<(), String> {
    if let Some(errs) = v.get("errors").and_then(|e| e.as_array()) {
        for e in errs {
            let t = e["type"].as_str().unwrap_or("");
            let m = e["message"].as_str().unwrap_or("");
            if t == "INSUFFICIENT_SCOPES" || m.contains("INSUFFICIENT_SCOPES") {
                return Err(ERR_MISSING_PROJECT_SCOPE.into());
            }
        }
    }
    Ok(())
}

fn errors_text(v: &Value) -> String {
    v.get("errors").map(|e| e.to_string()).unwrap_or_default()
}

const PROJECT_NODE_FIELDS: &str =
    "id number title shortDescription url closed updatedAt";

fn parse_project(p: &Value, owner_login: &str, owner_type: &str) -> ProjectV2Info {
    ProjectV2Info {
        id: p["id"].as_str().unwrap_or("").into(),
        number: p["number"].as_u64().unwrap_or(0),
        title: p["title"].as_str().unwrap_or("").into(),
        short_description: p["shortDescription"].as_str().unwrap_or("").into(),
        url: p["url"].as_str().unwrap_or("").into(),
        closed: p["closed"].as_bool().unwrap_or(false),
        updated_at: p["updatedAt"].as_str().unwrap_or("").into(),
        owner_login: owner_login.into(),
        owner_type: owner_type.into(),
    }
}

/// 从 user/organization/viewer 节点提取 projectsV2 列表；节点为 null 返回 None
fn projects_of(node: &Value, owner_type: &str) -> Option<Vec<ProjectV2Info>> {
    if node.is_null() {
        return None;
    }
    let login = node["login"].as_str().unwrap_or("");
    Some(
        node["projectsV2"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|p| parse_project(p, login, owner_type))
            .collect(),
    )
}

/// 列出某 owner 的 Projects V2。owner 为空 = 当前登录用户（viewer）；
/// ownerType 给 "user"/"org" 时走单一路径，否则两条路径都查：user 侧列表非空用 user，否则看 org
#[tauri::command]
pub async fn list_projects_v2(
    state: State<'_, AppState>,
    owner: Option<String>,
    owner_type: Option<String>,
) -> Result<Vec<ProjectV2Info>, String> {
    let token = ensure_token(&state)?;
    check_known_scope(&state)?;
    let owner = owner.unwrap_or_default();
    let owner_type = owner_type.unwrap_or_default();

    if owner.is_empty() {
        let q = format!(
            "query{{viewer{{login projectsV2(first:50,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{{PROJECT_NODE_FIELDS}}}}}}}}}"
        );
        let v = gql(&state.http, &token, &q, json!({})).await?;
        check_scope_errors(&v)?;
        return projects_of(&v["data"]["viewer"], "user")
            .ok_or_else(|| format!("GraphQL 错误: {}", errors_text(&v)));
    }

    let path = |kind: &str| {
        format!(
            "{kind}(login:$login){{login projectsV2(first:50,orderBy:{{field:UPDATED_AT,direction:DESC}}){{nodes{{{PROJECT_NODE_FIELDS}}}}}}}"
        )
    };
    match owner_type.as_str() {
        "user" | "org" => {
            let kind = if owner_type == "org" { "organization" } else { "user" };
            let q = format!("query($login:String!){{{}}}", path(kind));
            let v = gql(&state.http, &token, &q, json!({"login": owner})).await?;
            check_scope_errors(&v)?;
            return projects_of(&v["data"][kind], owner_type.as_str())
                .ok_or_else(|| format!("未找到 {owner_type} {owner} 或无权访问: {}", errors_text(&v)));
        }
        _ => {}
    }

    // 未指定类型：user / organization 两条路径合并查询。user 侧列表非空用 user，
    // 否则看 org（同名 user/org 且 project 都在 org 的场景）；org 节点存在即返回（哪怕空列表）。
    // 注意：另一侧可能因 org 级 PAT 限制报错，只要有一侧出数据就不算失败。
    let q = format!(
        "query($login:String!){{{} {}}}",
        path("user"),
        path("organization")
    );
    let v = gql(&state.http, &token, &q, json!({"login": owner})).await?;
    let user = projects_of(&v["data"]["user"], "user");
    let org = projects_of(&v["data"]["organization"], "org");
    match (user, org) {
        (Some(u), _) if !u.is_empty() => Ok(u),
        (_, Some(o)) => Ok(o),
        (Some(u), None) => Ok(u), // user 节点存在但 0 个 project、org 不存在
        (None, None) => {
            check_scope_errors(&v)?;
            Err(format!("未找到 owner {owner} 或无权访问: {}", errors_text(&v)))
        }
    }
}

fn parse_fields(nodes: &[Value]) -> Vec<ProjectV2Field> {
    nodes
        .iter()
        .map(|f| ProjectV2Field {
            id: f["id"].as_str().unwrap_or("").into(),
            name: f["name"].as_str().unwrap_or("").into(),
            data_type: f["dataType"].as_str().unwrap_or("").into(),
            options: f["options"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|o| ProjectV2FieldOption {
                    id: o["id"].as_str().unwrap_or("").into(),
                    name: o["name"].as_str().unwrap_or("").into(),
                })
                .collect(),
        })
        .collect()
}

fn parse_items(nodes: &[Value]) -> Vec<ProjectV2Item> {
    nodes
        .iter()
        .map(|i| {
            let c = &i["content"];
            let status = &i["status"];
            ProjectV2Item {
                id: i["id"].as_str().unwrap_or("").into(),
                content_type: c["__typename"].as_str().unwrap_or("").into(),
                title: c["title"].as_str().unwrap_or("").into(),
                number: c["number"].as_u64(),
                state: c["state"].as_str().unwrap_or("").into(),
                url: c["url"].as_str().unwrap_or("").into(),
                repo: c["repository"]["nameWithOwner"].as_str().unwrap_or("").into(),
                status: status["name"].as_str().map(|s| s.to_string()),
                status_option_id: status["optionId"].as_str().map(|s| s.to_string()),
                updated_at: i["updatedAt"].as_str().unwrap_or("").into(),
            }
        })
        .collect()
}

/// item 的 content 部分；status 部分单独拼（字段名走变量注入，见 items_query）
const ITEM_CONTENT_FIELDS: &str = r#"
id updatedAt
content{
  __typename
  ... on Issue{ number title state url repository{ nameWithOwner } }
  ... on PullRequest{ number title state url repository{ nameWithOwner } }
  ... on DraftIssue{ title }
}
"#;

const STATUS_SELECTION: &str = r#"
status: fieldValueByName(name:$statusField){
  __typename
  ... on ProjectV2ItemFieldSingleSelectValue{ name optionId }
}
"#;

/// Status 单选字段可被用户重命名：优先名为 "Status" 的 SINGLE_SELECT，否则取第一个 SINGLE_SELECT
fn status_field_name(fields: &[ProjectV2Field]) -> Option<String> {
    fields
        .iter()
        .find(|f| f.data_type == "SINGLE_SELECT" && f.name == "Status")
        .or_else(|| fields.iter().find(|f| f.data_type == "SINGLE_SELECT"))
        .map(|f| f.name.clone())
}

/// items 查询（所有分页共用）；with_status 时通过 $statusField 变量指定单选字段名，避免字符串拼接注入
fn items_query(with_status: bool) -> String {
    let mut q = String::from("query($id:ID!,$cursor:String");
    if with_status {
        q.push_str(",$statusField:String!");
    }
    q.push_str("){node(id:$id){... on ProjectV2{items(first:100,after:$cursor){totalCount pageInfo{hasNextPage endCursor} nodes{");
    q.push_str(ITEM_CONTENT_FIELDS);
    if with_status {
        q.push_str(STATUS_SELECTION);
    }
    q.push_str("}}}}}");
    q
}

/// 读取单个 project 的字段（含 Status 单选）与 items。内存缓存 5 分钟，refresh=true 强制刷新
#[tauri::command]
pub async fn get_project_v2(
    state: State<'_, AppState>,
    project_id: String,
    refresh: Option<bool>,
) -> Result<ProjectV2Board, String> {
    let token = ensure_token(&state)?;
    check_known_scope(&state)?;
    let key = format!("{}:{project_id}", token_fp(&token));
    if refresh != Some(true) {
        if let Some(c) = state.projects_v2_cache.lock().unwrap().as_ref() {
            if c.key == key && c.fetched_at.elapsed() < CACHE_TTL {
                return Ok(c.board.clone());
            }
        }
    }

    // 第一次：project 元信息 + 字段定义（小查询，用于定位 Status 单选字段的实际名字——该字段可被重命名）
    let qf = format!(
        r#"query($id:ID!){{node(id:$id){{... on ProjectV2{{
  {PROJECT_NODE_FIELDS}
  owner{{__typename ...on User{{login}} ...on Organization{{login}}}}
  fields(first:50){{nodes{{
    __typename
    ... on ProjectV2FieldCommon{{id name dataType}}
    ... on ProjectV2SingleSelectField{{options{{id name}}}}
  }}}}
}}}}}}"#
    );
    let v = gql(&state.http, &token, &qf, json!({"id": project_id})).await?;
    check_scope_errors(&v)?;
    let node = &v["data"]["node"];
    if node.is_null() {
        return Err(format!("未找到 project {project_id} 或无权访问: {}", errors_text(&v)));
    }
    let owner_type = match node["owner"]["__typename"].as_str() {
        Some("Organization") => "org",
        _ => "user",
    };
    let project = parse_project(node, node["owner"]["login"].as_str().unwrap_or(""), owner_type);
    let fields = parse_fields(&node["fields"]["nodes"].as_array().cloned().unwrap_or_default());
    let status_field = status_field_name(&fields);

    // items：所有页共用一个 query（带可选 $statusField）
    let qi = items_query(status_field.is_some());
    let mk_vars = |cursor: Option<String>| {
        let mut vars = json!({"id": project_id, "cursor": cursor});
        if let Some(sf) = &status_field {
            vars["statusField"] = json!(sf);
        }
        vars
    };
    let v = gql(&state.http, &token, &qi, mk_vars(None)).await?;
    check_scope_errors(&v)?;
    let conn = &v["data"]["node"]["items"];
    if conn.is_null() {
        return Err(format!("无法读取 project items: {}", errors_text(&v)));
    }
    let total_count = conn["totalCount"].as_u64().unwrap_or(0);
    let mut items = parse_items(&conn["nodes"].as_array().cloned().unwrap_or_default());
    let mut page_info = conn["pageInfo"].clone();

    // 后续页（总共最多 MAX_ITEM_PAGES 页）
    for _ in 1..MAX_ITEM_PAGES {
        if page_info["hasNextPage"].as_bool() != Some(true) {
            break;
        }
        let cursor = page_info["endCursor"].as_str().map(|s| s.to_string());
        let v = gql(&state.http, &token, &qi, mk_vars(cursor)).await?;
        check_scope_errors(&v)?;
        let conn = &v["data"]["node"]["items"];
        if conn.is_null() {
            break;
        }
        items.extend(parse_items(&conn["nodes"].as_array().cloned().unwrap_or_default()));
        page_info = conn["pageInfo"].clone();
    }
    let truncated = total_count > items.len() as u64;

    let board = ProjectV2Board { project, fields, items, total_count, truncated };
    *state.projects_v2_cache.lock().unwrap() = Some(BoardCache {
        key,
        board: board.clone(),
        fetched_at: std::time::Instant::now(),
    });
    Ok(board)
}
