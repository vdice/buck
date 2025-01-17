#[macro_use]
extern crate serde_json;
extern crate base64;
extern crate ulid;

use kube::{
    api::{Api, Informer, Object, PostParams, RawApi, WatchEvent},
    client::APIClient,
    config,
};

type KubeObj = Object<serde_json::Value, serde_json::Value>;

fn main() {
    let kubeconfig = config::load_kube_config().expect("kubeconfig failed to load");
    let client = APIClient::new(kubeconfig);
    let namespace = Some(std::env::var("NAMESPACE").unwrap_or_else(|_| "default".into()));
    let project = std::env::var("PROJECT").expect("PROJECT env var is required");

    // default is pod/v1
    let group = std::env::var("GROUP").unwrap_or_else(|_| "".into());
    let version = std::env::var("VERSION").unwrap_or_else(|_| "v1".into());
    let kind = std::env::var("KIND").unwrap_or_else(|_| "pod".into());
    let plural = std::env::var("PLURAL").unwrap_or_else(|_| "pods".into());
    let prefix = std::env::var("PREFIX")
        .or_else(|e| {
            if kind == "pod" {
                return Ok("api".to_string());
            }
            Err(e)
        })
        .unwrap_or_else(|_| "apis".into())
        .to_string();

    let resource = RawApi {
        namespace: namespace.clone(),
        group,
        version,
        prefix,
        resource: plural,
    };

    // Create our informer and start listening.
    let informer = Informer::raw(client.clone(), resource)
        .init()
        .expect("informer init failed");
    loop {
        informer.poll().expect("informer poll failed");
        while let Some(event) = informer.pop() {
            
            handle(client.clone(), event, project.clone(), namespace.clone().unwrap());
        }
    }
}

fn handle(client: APIClient, event: WatchEvent<KubeObj>, project: String, namespace: String) {
    match event {
        WatchEvent::Added(o) => create_secret(client, "resource_added", o, project, namespace),
        WatchEvent::Modified(o) => create_secret(client, "resource_modified", o, project, namespace),
        WatchEvent::Deleted(o) => create_secret(client, "resource_delete", o, project, namespace),
        WatchEvent::Error(e) => println!("Error: {}", e),
    }
}

fn create_secret(client: APIClient, event: &str, payload: KubeObj, project: String, namespace: String) {
    println!("Event {} on resource {}", event, payload.metadata.name);
    let uid = ulid::Ulid::new().to_string().to_ascii_lowercase();
    let name = format!("buck-{}", uid);
    let pp = PostParams::default();
    let encoded_payload = serde_json::to_string(&payload).unwrap_or_else(|_| "".to_string());
    let secret = json!({
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {
            "name": name,
            "labels": {
                "heritage": "brigade",
                "project": project.as_str(),
                "build": uid.as_str(),
                "component": "build"
            }
        },
        "type": "brigade.sh/build",
        "data": {
            "event_provider": base64::encode("buck"),
            "event_type": base64::encode(event),
            "project_id": base64::encode(project.as_str()),
            "build_id": base64::encode(uid.as_str()),
            "payload": base64::encode(encoded_payload.as_str())
        }
    });

    let data = serde_json::to_vec(&secret);
    println!("{}", serde_json::to_string_pretty(&secret).unwrap());
    if data.is_err() {
        println!("Error serializing secret: {}", data.unwrap_err());
        return;
    }

    match Api::v1Secret(client)
        .within(namespace.as_str())
        .create(&pp, data.unwrap())
    {
        Ok(_) => println!("Sent Brigade event"),
        Err(e) => println!("Error sending event: {}", e),
    };
}
