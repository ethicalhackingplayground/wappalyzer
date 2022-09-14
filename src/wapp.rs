use serde::de;
use serde::{Deserialize, Deserializer, Serialize};

use futures::future::join_all;
use regex::Regex;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

extern crate lazy_static;

// The build.rs build script reads the local apps.json file at build time
// and includes the text string as a constant in a created 'apps.json.rs'
// in the build dir. Here, we include this constant.
include!(concat!(env!("OUT_DIR"), "/apps.json.rs"));

/// A very simple representation for cookie data
#[derive(Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug)]
pub struct RawData {
    pub headers: HashMap<String, String>,
    pub cookies: Vec<Cookie>,
    pub meta_tags: HashMap<String, String>,
    pub script_tags: Vec<String>,
    pub html: String,
}

pub async fn check(raw_data: Arc<RawData>) -> Vec<Tech> {
    let mut futures: Vec<tokio::task::JoinHandle<Option<Vec<Tech>>>> = vec![];

    for app in APPS_JSON_DATA.apps.values() {
        futures.push(app.tech_tokio(raw_data.clone()));
    }

    join_all(futures)
        .await
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .filter(|o| o.is_some())
        .map(|r| r.as_ref().unwrap().to_owned())
        .flat_map(|a| a.into_iter())
        .collect::<Vec<_>>()
}

lazy_static! {
    static ref APPS_JSON_DATA: AppsJsonData = {
        let mut apps_json_data: AppsJsonData =
            serde_json::from_str(APPS_JSON_TEXT).expect("Error loading the apps.json file");

        for (app_name, app) in apps_json_data.apps.iter_mut() {
            (*app).name = String::from(app_name);
        }

        apps_json_data
    };
}

/// A technology that is found on a page
#[derive(Debug, PartialEq, Eq, Hash,  Clone, Serialize, Deserialize)]
pub struct Tech {
    pub category: String,
    pub name: String,
}
impl Tech {
    /// let tech = Tech::named("webpack");
    /// assert_eq!(tech.name, "webpack");
    /// assert_eq!(tech.category, "Miscellaneous");
    // fn named(name: &str) -> Option<Tech> {
    //     if let Some(app) = APPS_JSON_DATA.named(name) {
    //         Some(Tech::from(app))
    //     } else {
    //         None
    //     }
    // }

    pub fn from(app: &App) -> Tech {
        Tech {
            name: app.name.clone(),
            category: app.category_name(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppsJsonData {
    apps: HashMap<String, App>,
    categories: HashMap<u32, Category>,
}
impl AppsJsonData {
    // fn named(&self, name: &str) -> Option<&App> {
    //     self.apps.get(&String::from(name))
    // }

    fn category_name(&self, id: u32) -> Option<String> {
        match self.categories.get(&id) {
            // Some(category) => Some(String::from(category.name)),
            Some(category) => Some(category.name.clone()),
            None => None,
        }
    }
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct App {
    #[serde(skip)]
    name: String,
    cats: Vec<u32>,
    website: String,
    #[serde(default)]
    priority: i32,
    #[serde(deserialize_with = "one_or_more_strings")]
    #[serde(default)]
    html: Vec<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    cookies: HashMap<String, String>,
    #[serde(default)]
    js: HashMap<String, String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    meta: HashMap<String, String>,
    #[serde(default)]
    icon: String,
    #[serde(deserialize_with = "one_or_more_strings")]
    #[serde(default)]
    implies: Vec<String>,
    #[serde(default)]
    #[serde(deserialize_with = "one_or_more_strings")]
    excludes: Vec<String>,
    #[serde(default)]
    #[serde(deserialize_with = "one_or_more_strings")]
    script: Vec<String>,
}

impl App {
    pub fn category_name(&self) -> String {
        assert!(!self.cats.is_empty());
        APPS_JSON_DATA.category_name(self.cats[0]).unwrap()
    }

    // pub fn check_headers(&self,)
    // pub async fn tech(
    //     &self,
    //     headers: &reqwest::header::HeaderMap,
    //     cookies: &[crate::Cookie],
    //     meta_tags: &HashMap<String, String>,
    //     parsed_html: &Html,
    //     html: &str,
    // ) -> Option<Tech> {
    //     if self.check(headers.clone(), cookies.clone(), meta_tags.clone(), html.clone()) {
    //         Some(Tech::from(self))
    //     } else {
    //         None
    //     }
    // }

    pub fn tech_tokio(
        &'static self,
        raw_data: Arc<RawData>,
    ) -> tokio::task::JoinHandle<Option<Vec<Tech>>> {
        tokio::spawn(async move {
            if self.check(raw_data.clone()) {
                let mut tech = vec![];
                for i in &self.implies {
                    tech.push(Tech::from(APPS_JSON_DATA.apps.get(i)?));
                }
                tech.push(Tech::from(self));
                Some(tech)
            } else {
                None
            }
        })
    }

    // TODO: initially only checking for one positive
    pub fn check(&self, raw_data: Arc<RawData>) -> bool {
        // check headers
        for (header_to_check, expected_value) in self.headers.iter() {

            if let Some(value) = raw_data.headers.get(header_to_check.to_lowercase().as_str()) {
                   if check_text(expected_value, value.as_str()) {
                        //eprintln!(
                        //    "||| HEADER ({}) hit on: {}",
                        //    header_to_check, expected_value
                        //);
                        return true;
                }
            }
        }

        // html
        for maybe_regex in self.html.iter() {
            if check_text(maybe_regex, &raw_data.html) {
                // eprintln!("||| HTML hit on: {}", maybe_regex);
                return true; // TODO: temp impletation that returns on any hit
            }
        }

        // cookies
        for (cookies_to_check, expected_value) in self.cookies.iter() {
            // Examples from app.json
            // "__cfduid": ""
            // "__derak_auth": "",
            // "_session_id": "\\;confidence:75"
            // "ci_csrf_token": "^(.+)$\\;version:\\1?2+:",
            // "Fe26.2**": "\\;confidence:50"

            // COOKIE: Cookie { cookie_string: Some("1P_JAR=2019-09-18-19; expires=Fri, 18-Oct-2019 19:05:14 GMT; path=/; domain=.google.com; SameSite=none"), name: Indexed(0, 6), value: Indexed(7, 20), expires: Some(Tm { tm_sec: 14, tm_min: 5, tm_hour: 19, tm_mday: 18, tm_mon: 9, tm_year: 119, tm_wday: 5, tm_yday: 0, tm_isdst: 0, tm_utcoff: 0, tm_nsec: 0 }), max_age: None, domain: Some(Indexed(77, 87)), path: Some(Indexed(66, 67)), secure: None, http_only: None, same_site: None }
            // COOKIE: Cookie { cookie_string: Some("NID=188=E7jfAOxVZYeABbEwAi-4RN6pK1a-98zWM1hcFnt8bjHM_303Gon7qmJCopif_taWAwwNrpB9bcjQXn1Mm9gRzIagJSoLll4Wp0XHrPtBUMIXN58jCbdQFVEKAz1yJgyi_oxdG6NVYB2An8_RWmJ-EWp-6umHMMatZfxTAyE2-n8; expires=Thu, 19-Mar-2020 19:05:14 GMT; path=/; domain=.google.com; HttpOnly"), name: Indexed(0, 3), value: Indexed(4, 179), expires: Some(Tm { tm_sec: 14, tm_min: 5, tm_hour: 19, tm_mday: 19, tm_mon: 2, tm_year: 120, tm_wday: 4, tm_yday: 0, tm_isdst: 0, tm_utcoff: 0, tm_nsec: 0 }), max_age: None, domain: Some(Indexed(236, 246)), path: Some(Indexed(225, 226)), secure: None, http_only: Some(true), same_site: None }

            // loop through and find the appropriate cookie
            if let Some(c) = raw_data.cookies.iter().find(|c| {
                // eprintln!("COOKIE: ({})==({})", c.name(), cookies_to_check);
                c.name == *cookies_to_check
            }) {
                // an empty expected_value means that we only care about the existence if the cookie
                if expected_value.is_empty() || check_text(expected_value, &c.value) {
                    // eprintln!("||| COOKIE ({}) hit on: {}", c.value, expected_value);
                    return true; // TODO: Temp impl where one hit returns
                }
            }
        }

        // try just checking for the js_to_check value, as (1) the js version seems to use the dom directly, and
        // (2) the Go version doesn't seem to work
        for (js_to_check, _rule_value) in self.js.iter() {
            for js in &raw_data.script_tags {
                if check_text(js_to_check, js) {
                    // eprintln!("||| JS hit on: {}", js_to_check);
                    return true;
                }
            }
        }

        // meta
        for (meta_to_check, expected_value) in self.meta.iter() {
            if let Some(value) = raw_data.meta_tags.get(meta_to_check) {
                // an empty expected_value means that we only care about the existence if the cookie
                if check_text(expected_value, value) {
                    // eprintln!(
                    //     "||| META ({}) hit on: {} for value: {}",
                    //     meta_to_check, expected_value, value
                    // );
                    return true; // TODO: Temp impl where one hit returns
                }
            }
        }

        // check html
        false
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Category {
    name: String,
    priority: u8,
}

// The meat of the matter
fn check_text(maybe_regex: &str, text: &str) -> bool {
    // TODO: strignoring version stuff for now.
    // TODO: Compile regex's in the initialization area.
    let maybe_regex = format!("(?i){}", maybe_regex);
    let maybe_regex = maybe_regex.split("\\;").collect::<Vec<&str>>()[0];
    match Regex::new(maybe_regex) {
        Ok(re) => {
            //println!("REGEX IS FINE: [{}] - trying on [{}] and got {:?}", maybe_regex, text, re.is_match(text));

            re.is_match(text)
        }
        Err(_) => {
             //eprintln!("invalid regex in app.json '{}': {}", maybe_regex, err);
             //panic!("invalid regex in app.json '{}': {}", maybe_regex, err);
             false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use reqwest::header;

    // #[test]
    // fn tech_lookup() {
    //     let tech = Tech::named("webpack").unwrap();
    //     assert_eq!(tech.name, "webpack");
    //     assert_eq!(tech.category, "Miscellaneous");
    // }

    // #[test]
    // fn test_check_app() {
    //     // assert_eq!(
    //     //     APPS_JSON_DATA
    //     //         .named("webpack")
    //     //         .unwrap()
    //     //         .tech(&header::HeaderMap::new(), ""),
    //     //     None
    //     // );
    //     // assert_eq!(
    //     //     APPS_JSON_DATA.named("webpack").unwrap().tech(""),
    //     //     Tech::named("webpack")
    //     // );
    // }

    #[test]
    fn test_check_text() {
        assert!(check_text("foo", "somefood"));
        assert!(!check_text("bar", "somefood"));
        assert!(check_text("[CK]amva", "Kamva"));
        assert!(!check_text("[CK]amva", "Lamva"));
        assert!(check_text(
            "cf\\.kampyle\\.com/k_button\\.js",
            "some cf.kampyle.com/k_button.js"
        ));
        assert!(!check_text(
            "cf\\.kampyle\\.com/k_button\\.js",
            "some cXf.kampyle.com/k_button.js"
        ));
        assert!(check_text(
            "optimizely\\.com.*\\.js",
            "cdn.optimizely.com/js/711892001.js"
        ));
        assert!(!check_text(
            "<link[^>]+?href=[^\"]/css/([\\d.]+)/bootstrap\\.(?:min\\.)?css\\;version:\\1",
            "cdn.optimizely.com/js/711892001.js"
        ));

        //         invalid regex in app.json '<link[^>]+?href=[^"]/css/([\d.]+)/bootstrap\.(?:min\.)?css\;version:\1': regex parse error:
        // <link[^>]+?href=[^"]/css/([\d.]+)/bootstrap\.(?:min\.)?css\;version:\1

        // assert!(!check_text(
        //     "<link[^>]*\\s+href=[^>]*styles/kendo\\.common(?:\\.min)?\\.css[^>]*/>",
        //     ""
        // ));
        // assert!(check_text(
        //     "<link[^>]*\\s+href=[^>]*styles/kendo\\.common(?:\\.min)?\\.css[^>]*/>",
        //     "<link "
        // ));
    }
}

fn one_or_more_strings<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrVec(PhantomData<Vec<String>>);

    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or list of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<S>(self, visitor: S) -> Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(visitor))
        }
    }

    deserializer.deserialize_any(StringOrVec(PhantomData))
}
