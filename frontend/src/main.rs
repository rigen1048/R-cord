use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

async fn cmd<T: for<'de> Deserialize<'de>>(name: &str, args: &impl Serialize) -> Result<T, String> {
    let js = invoke(name, serde_wasm_bindgen::to_value(args).unwrap())
        .await
        .map_err(|e| format!("{e:?}"))?;
    serde_wasm_bindgen::from_value(js).map_err(|e| e.to_string())
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
struct TreeNode {
    name: String,
    path: String,
    children: Vec<TreeNode>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct Take {
    name: String,
    path: String,
    duration_secs: Option<f64>,
}

#[derive(Deserialize, Clone, Debug)]
struct DeviceInfo {
    name: String,
    is_default: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Empty {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetVaultArgs {
    path: String,
}

fn fmt_duration(secs: Option<f64>) -> String {
    match secs {
        Some(s) => format!("{}:{:02}", (s as u64) / 60, (s as u64) % 60),
        None => "--:--".into(),
    }
}

#[component]
fn App() -> impl IntoView {
    let vault: RwSignal<Option<Option<String>>> = RwSignal::new(None); // outer None = loading
    spawn_local(async move {
        let v: Option<String> = cmd("get_vault", &Empty {}).await.unwrap_or(None);
        vault.set(Some(v));
    });

    view! {
        <div class="shell">
            {move || match vault.get() {
                None => view! { <div class="vault-picker"><p>"Loading…"</p></div> }.into_any(),
                Some(None) => view! { <VaultPicker on_picked=move |p| vault.set(Some(Some(p))) /> }.into_any(),
                Some(Some(path)) => view! { <Main vault=path /> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn VaultPicker(on_picked: impl Fn(String) + 'static + Copy) -> impl IntoView {
    let error = RwSignal::new(String::new());
    let pick = move |_| {
        spawn_local(async move {
            // open folder dialog via tauri dialog plugin
            let js = invoke(
                "plugin:dialog|open",
                serde_wasm_bindgen::to_value(&serde_json::json!({
                    "directory": true,
                    "title": "Choose your Rcord vault folder"
                }))
                .unwrap(),
            )
            .await;
            match js {
                Ok(v) => {
                    let path: Option<String> = serde_wasm_bindgen::from_value(v).unwrap_or(None);
                    if let Some(p) = path {
                        match cmd::<()>("set_vault", &SetVaultArgs { path: p.clone() }).await {
                            Ok(()) => on_picked(p),
                            Err(e) => error.set(e),
                        }
                    }
                }
                Err(e) => error.set(format!("{e:?}")),
            }
        });
    };
    view! {
        <div class="vault-picker">
            <h1>"Rcord"</h1>
            <p>"Choose a vault folder — your recordings live there as plain files."</p>
            <button class="action" on:click=pick>"Choose vault folder"</button>
            <p style="color: var(--accent)">{move || error.get()}</p>
        </div>
    }
}

#[component]
fn TreeView(
    nodes: Vec<TreeNode>,
    selected: RwSignal<String>,
    refresh: RwSignal<u32>,
) -> AnyView {
    nodes
        .into_iter()
        .map(|n| {
            let path = n.path.clone();
            let path2 = n.path.clone();
            let renaming = RwSignal::new(false);
            let rename_val = RwSignal::new(n.name.clone());
            let kids = n.children.clone();
            view! {
                <div>
                    <div
                        class="tree-item"
                        class:selected={
                            let psel = path.clone();
                            move || selected.get() == psel
                        }
                        on:click={
                            let p = path.clone();
                            move |_| selected.set(p.clone())
                        }
                        on:dblclick=move |_| renaming.set(true)
                    >
                        <span class="icon">"📁"</span>
                        {move || {
                            if renaming.get() {
                                view! {
                                    <input
                                        prop:value=rename_val
                                        on:input=move |ev| rename_val.set(event_target_value(&ev))
                                        on:keydown={
                                            let path3 = path2.clone();
                                            move |ev| {
                                            if ev.key() == "Enter" {
                                                let p = path3.clone();
                                                let new = rename_val.get_untracked();
                                                spawn_local(async move {
                                                    let _: Result<(), String> = cmd(
                                                        "rename_folder",
                                                        &serde_json::json!({ "path": p, "newName": new }),
                                                    )
                                                    .await;
                                                    renaming.set(false);
                                                    refresh.update(|r| *r += 1);
                                                });
                                            }
                                            if ev.key() == "Escape" { renaming.set(false); }
                                            }
                                        }
                                        on:click=|ev| ev.stop_propagation()
                                        on:dblclick=|ev| ev.stop_propagation()
                                    />
                                }
                                    .into_any()
                            } else {
                                view! { <span>{n.name.clone()}</span> }.into_any()
                            }
                        }}
                    </div>
                    <div class="tree-children">
                        <TreeView nodes=kids selected=selected refresh=refresh />
                    </div>
                </div>
            }
        })
        .collect_view()
        .into_any()
}

#[component]
fn Main(vault: String) -> impl IntoView {
    let tree: RwSignal<Vec<TreeNode>> = RwSignal::new(vec![]);
    let selected: RwSignal<String> = RwSignal::new(vault.clone());
    let refresh: RwSignal<u32> = RwSignal::new(0);
    let takes: RwSignal<Vec<Take>> = RwSignal::new(vec![]);
    let devices: RwSignal<Vec<DeviceInfo>> = RwSignal::new(vec![]);
    let device_choice: RwSignal<Option<String>> = RwSignal::new(None);
    let recording = RwSignal::new(false);
    let rec_secs = RwSignal::new(0u64);
    let last_take: RwSignal<Option<Take>> = RwSignal::new(None);

    let vault_name = std::path::Path::new(&vault)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| vault.clone());

    // load tree whenever refresh bumps
    let v = vault.clone();
    Effect::new(move |_| {
        refresh.get();
        let v = v.clone();
        spawn_local(async move {
            if let Ok(t) = cmd::<Vec<TreeNode>>("list_tree", &serde_json::json!({ "vault": v })).await {
                tree.set(t);
            }
        });
    });

    // load takes when selection changes or refresh bumps
    Effect::new(move |_| {
        refresh.get();
        let folder = selected.get();
        spawn_local(async move {
            if let Ok(t) = cmd::<Vec<Take>>("list_takes", &serde_json::json!({ "folder": folder })).await {
                takes.set(t);
            }
        });
    });

    // load devices once
    spawn_local(async move {
        if let Ok(d) = cmd::<Vec<DeviceInfo>>("list_input_devices", &Empty {}).await {
            device_choice.set(
                d.iter().find(|d| d.is_default).map(|d| d.name.clone()),
            );
            devices.set(d);
        }
    });

    let new_folder = move |_| {
        let parent = selected.get_untracked();
        spawn_local(async move {
            let _: Result<TreeNode, String> = cmd(
                "create_folder",
                &serde_json::json!({ "parent": parent, "name": "New Folder" }),
            )
            .await;
            refresh.update(|r| *r += 1);
        });
    };

    let toggle_rec = move |_| {
        if recording.get_untracked() {
            spawn_local(async move {
                match cmd::<Take>("stop_recording", &Empty {}).await {
                    Ok(t) => {
                        recording.set(false);
                        last_take.set(Some(t));
                        refresh.update(|r| *r += 1);
                    }
                    Err(_) => recording.set(false),
                }
            });
        } else {
            let folder = selected.get_untracked();
            let dev = device_choice.get_untracked();
            spawn_local(async move {
                match cmd::<()>(
                    "start_recording",
                    &serde_json::json!({ "folder": folder, "device": dev }),
                )
                .await
                {
                    Ok(()) => {
                        rec_secs.set(0);
                        recording.set(true);
                        // simple timer
                        while recording.get_untracked() {
                            gloo_timer(1000).await;
                            if recording.get_untracked() {
                                rec_secs.update(|s| *s += 1);
                            }
                        }
                    }
                    Err(e) => {
                        leptos::logging::error!("start_recording failed: {e}");
                    }
                }
            });
        }
    };

    view! {
        <div class="main">
            <div class="sidebar">
                <div class="vault-name">{vault_name}</div>
                <div class="tree">
                    <div
                        class="tree-item"
                        class:selected={
                            let value = vault.clone();
                            move || selected.get() == value
                        }
                        on:click={
                            let v = vault.clone();
                            move |_| selected.set(v.clone())
                        }
                    >
                        <span class="icon">"🗂"</span>
                        <span>"(vault root)"</span>
                    </div>
                    {move || {
                        view! { <TreeView nodes=tree.get() selected=selected refresh=refresh /> }
                    }}
                </div>
                <button class="new-folder" on:click=new_folder>"+ New folder (in selected)"</button>
            </div>
            <div class="content">
                <div class="folder-view">
                    <h2>{move || {
                        let s = selected.get();
                        std::path::Path::new(&s)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or(s)
                    }}</h2>
                    <div class="take-list">
                        {move || {
                            let ts = takes.get();
                            if ts.is_empty() {
                                view! { <p class="empty">"No recordings here yet. Hit the red button."</p> }.into_any()
                            } else {
                                ts.into_iter()
                                    .map(|t| {
                                        let path = t.path.clone();
                                        let name = RwSignal::new(t.name.clone());
                                        let path2 = t.path.clone();
                                        let is_last = last_take.get().map(|lt| lt.path == t.path).unwrap_or(false);
                                        let input_ref = NodeRef::<leptos::html::Input>::new();
                                        if is_last {
                                            // highlight auto-named fresh take for immediate rename
                                            leptos::prelude::set_timeout(
                                                move || {
                                                    if let Some(el) = input_ref.get() {
                                                        let _ = el.focus();
                                                        el.select();
                                                    }
                                                },
                                                std::time::Duration::from_millis(50),
                                            );
                                        }
                                        view! {
                                            <div class="take">
                                                <button
                                                    class="play"
                                                    on:click={
                                                        let p = path2.clone();
                                                        move |_| {
                                                            let p = p.clone();
                                                            spawn_local(async move {
                                                                let _: Result<(), String> = cmd(
                                                                    "play_take",
                                                                    &serde_json::json!({ "path": p }),
                                                                )
                                                                .await;
                                                            });
                                                        }
                                                    }
                                                >
                                                    "▶"
                                                </button>
                                                <input
                                                    node_ref=input_ref
                                                    prop:value=name
                                                    on:input=move |ev| name.set(event_target_value(&ev))
                                                    on:keydown={
                                                        let p = path.clone();
                                                        move |ev| {
                                                            if ev.key() == "Enter" {
                                                                let p = p.clone();
                                                                let new = name.get_untracked();
                                                                spawn_local(async move {
                                                                    let _: Result<String, String> = cmd(
                                                                        "rename_take",
                                                                        &serde_json::json!({ "path": p, "newName": new }),
                                                                    )
                                                                    .await;
                                                                    refresh.update(|r| *r += 1);
                                                                });
                                                            }
                                                        }
                                                    }
                                                />
                                                <span class="dur">{fmt_duration(t.duration_secs)}</span>
                                            </div>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        }}
                    </div>
                </div>
                <div class="recorder">
                    <button
                        class="rec-btn"
                        class:recording=move || recording.get()
                        on:click=toggle_rec
                    ></button>
                    <span class="rec-time">
                        {move || format!("{}:{:02}", rec_secs.get() / 60, rec_secs.get() % 60)}
                    </span>
                    <select on:change=move |ev| {
                        let v = event_target_value(&ev);
                        device_choice.set(if v.is_empty() { None } else { Some(v) });
                    }>
                        {move || {
                            devices
                                .get()
                                .into_iter()
                                .map(|d| {
                                    let sel = device_choice.get().as_deref() == Some(d.name.as_str());
                                    view! {
                                        <option value=d.name.clone() selected=sel>
                                            {d.name.clone()}
                                        </option>
                                    }
                                })
                                .collect_view()
                        }}
                    </select>
                    <span class="rec-hint">
                        {move || {
                            if recording.get() {
                                "Recording into selected folder… press again to stop & auto-save".to_string()
                            } else {
                                "Select a folder, then record.".to_string()
                            }
                        }}
                    </span>
                </div>
            </div>
        </div>
    }
}

async fn gloo_timer(ms: u32) {
    let (tx, rx) = futures_channel::oneshot::channel::<()>();
    leptos::prelude::set_timeout(
        move || {
            let _ = tx.send(());
        },
        std::time::Duration::from_millis(ms as u64),
    );
    let _ = rx.await;
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
