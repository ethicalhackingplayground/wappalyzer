#[macro_use]
extern crate lazy_static;

pub mod wapp;

use headless_chrome::protocol::cdp::Network::GetResponseBodyReturnObject;
use headless_chrome::{Browser, Tab};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use url::Url;
use wapp::{RawData, Tech};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub url: String,
    pub result: Result<Vec<Tech>, String>,
}

/// Possible Errors in the domain_info lib
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WappError {
    Fetch(String),
    Analyze(String),
    Other(String),
}

impl fmt::Display for WappError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                WappError::Fetch(err) => format!("Fetch/{}", err),
                WappError::Analyze(err) => format!("Analyze/{}", err),
                WappError::Other(err) => format!("Other/{}", err),
            }
        )
    }
}

impl std::convert::From<std::io::Error> for WappError {
    fn from(err: std::io::Error) -> Self {
        WappError::Other(err.to_string())
    }
}
impl From<&dyn std::error::Error> for WappError {
    fn from(err: &dyn std::error::Error) -> Self {
        WappError::Other(err.to_string())
    }
}

// the trait `std::convert::From<std::str::Utf8Error>` is not implemented for `WappError`
impl From<std::str::Utf8Error> for WappError {
    fn from(err: std::str::Utf8Error) -> Self {
        WappError::Other(err.to_string())
    }
}

pub async fn scan(url: Url) -> Analysis {
    let url_str = String::from(url.as_str());
    match fetch(url).await {
        Ok(raw_data) => {
            let analysis = wapp::check(raw_data).await;
            Analysis {
                url: url_str,
                result: Ok(analysis),
            }
        }
        Err(err) => Analysis {
            url: url_str,
            result: Err(err.to_string()),
        },
    }
}

fn get_html(tab: &Tab) -> Option<String> {
    let remote_object = tab
        .evaluate("document.documentElement.outerHTML", false)
        .ok()?;

    let json = remote_object.value?;
    let str = json.as_str()?;

    Some(str.to_owned())
}

async fn fetch(url: Url) -> Result<Arc<wapp::RawData>, WappError> {
    let browser = Browser::default().unwrap();

    let tab = browser.wait_for_initial_tab().unwrap();

    let responses = Arc::new(Mutex::new(Vec::new()));
    let responses2 = responses.clone();

    tab.enable_response_handling(Box::new(move |response, fetch_body| {
        let body = fetch_body().unwrap_or(GetResponseBodyReturnObject {
            body: "".to_string(),
            base_64_encoded: false,
        });
        responses2.lock().unwrap().push((response, body));
    }))
    .unwrap();
    tab.navigate_to(url.as_str()).unwrap();

    let rendered_tab = tab.wait_until_navigated().unwrap();

    let html = get_html(rendered_tab).unwrap();

    let final_responses: Vec<_> = responses.lock().unwrap().clone();

    let headers: HashMap<String, String> = final_responses
        .into_iter()
        .nth(0)
        .unwrap()
        .0
        .response
        .headers
        .0
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
        .into_iter()
        .map(|(a, b)| (a.to_lowercase(), b.to_string().replace("\"", "")))
        .collect();
    println!("{:?}", headers);
     // Revisiting since cookies aren't always detected on first tab.
    let cookies: Vec<wapp::Cookie> = tab
        .navigate_to(url.as_str())
        .unwrap()
        .get_cookies()
        .unwrap()
        .into_iter()
        .map(|c| wapp::Cookie {
            name: c.name,
            value: c.value,
        })
        .collect();

    let parsed_html = Html::parse_fragment(&html);
    let selector = Selector::parse("meta").unwrap();
    let mut script_tags = vec![];
    for js in parsed_html.select(&Selector::parse("script").unwrap()) {
        script_tags.push(js.html());
    }

    // Note: using a hashmap will not support two meta tags with the same name and different values,
    // though I'm not sure if that's legal html.
    let mut meta_tags = HashMap::new();
    for meta in parsed_html.select(&selector) {
        if let (Some(name), Some(content)) =
            (meta.value().attr("name"), meta.value().attr("content"))
        {
            // eprintln!("META {} -> {}", name, content);
            meta_tags.insert(String::from(name), String::from(content));
        }
    }
    let raw_data = Arc::new(RawData {
        headers,
        cookies,
        meta_tags,
        script_tags,
        html,
    });

    Ok(raw_data)
}
