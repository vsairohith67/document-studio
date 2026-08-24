use std::ffi::{OsStr, OsString};

const WEBVIEW2_ENVIRONMENT_PREFIX: &str = "WEBVIEW2_";
const COREWEBVIEW2_MAX_INSTANCES_ENV: &str = "COREWEBVIEW2_MAX_INSTANCES";
const COREWEBVIEW2_MAX_INSTANCES_VALUE: &str = "20";

#[derive(Debug, PartialEq, Eq)]
struct Webview2EnvironmentPlan {
    inherited_keys_to_remove: Vec<OsString>,
    enforced_key: &'static str,
    enforced_value: &'static str,
}

fn is_managed_webview2_environment_key(key: &OsStr) -> bool {
    let key = key.to_string_lossy();
    key.get(..WEBVIEW2_ENVIRONMENT_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(WEBVIEW2_ENVIRONMENT_PREFIX))
        || key.eq_ignore_ascii_case(COREWEBVIEW2_MAX_INSTANCES_ENV)
}

fn plan_webview2_environment_policy<I>(inherited_keys: I) -> Webview2EnvironmentPlan
where
    I: IntoIterator<Item = OsString>,
{
    Webview2EnvironmentPlan {
        inherited_keys_to_remove: inherited_keys
            .into_iter()
            .filter(|key| is_managed_webview2_environment_key(key))
            .collect(),
        enforced_key: COREWEBVIEW2_MAX_INSTANCES_ENV,
        enforced_value: COREWEBVIEW2_MAX_INSTANCES_VALUE,
    }
}

pub(crate) fn enforce_webview2_environment_policy() {
    let plan = plan_webview2_environment_policy(std::env::vars_os().map(|(key, _)| key));
    for key in plan.inherited_keys_to_remove {
        std::env::remove_var(key);
    }
    std::env::set_var(plan.enforced_key, plan.enforced_value);
}

#[cfg(feature = "test-runtime")]
pub(crate) fn test_runtime_environment_evidence() -> Result<&'static [u8], &'static str> {
    let controlled_keys = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| is_managed_webview2_environment_key(key))
        .collect::<Vec<_>>();
    if controlled_keys != [OsString::from(COREWEBVIEW2_MAX_INSTANCES_ENV)] {
        return Err("inherited WebView2 environment controls survived startup policy");
    }
    if std::env::var_os(COREWEBVIEW2_MAX_INSTANCES_ENV).as_deref()
        != Some(OsStr::new(COREWEBVIEW2_MAX_INSTANCES_VALUE))
    {
        return Err("COREWEBVIEW2_MAX_INSTANCES does not equal 20");
    }
    Ok(b"setup reached\nWEBVIEW2_ENVIRONMENT=clean\nCOREWEBVIEW2_MAX_INSTANCES=20\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_every_managed_name_case_insensitively() {
        for key in [
            "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
            "webview2_user_data_folder",
            "WebView2_Additional_Browser_Arguments",
            "WEBVIEW2_CHANNEL_SEARCH_KIND",
            "WEBVIEW2_RELEASE_CHANNELS",
            "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
            "WEBVIEW2_DEFAULT_BACKGROUND_COLOR",
            "WeBvIeW2_FuTuRe_HOSTILE_OVERRIDE",
            "COREWEBVIEW2_MAX_INSTANCES",
            "corewebview2_max_instances",
            "CoreWebView2_Max_Instances",
        ] {
            assert!(
                is_managed_webview2_environment_key(OsStr::new(key)),
                "managed key was not classified: {key}"
            );
        }
        for key in [
            "DOCUMENT_STUDIO_TEST_WEBVIEW2_DATA_DIR",
            "DOCUMENT_STUDIO_TEST_CDP_PORT",
            "WEBVIEW2",
            "NOT_WEBVIEW2_USER_DATA_FOLDER",
            "COREWEBVIEW2_MAX_INSTANCES_SUFFIX",
        ] {
            assert!(
                !is_managed_webview2_environment_key(OsStr::new(key)),
                "unmanaged key was classified: {key}"
            );
        }
    }

    #[test]
    fn plans_complete_removal_before_the_single_exact_assignment() {
        let inherited = [
            "ordinary",
            "WEBVIEW2_USER_DATA_FOLDER",
            "webview2_browser_executable_folder",
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
            "webview2_future_remote_debug_variant",
            "CoReWeBvIeW2_MaX_InStAnCeS",
        ]
        .map(OsString::from);

        let plan = plan_webview2_environment_policy(inherited);
        assert_eq!(
            plan.inherited_keys_to_remove,
            [
                "WEBVIEW2_USER_DATA_FOLDER",
                "webview2_browser_executable_folder",
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
                "webview2_future_remote_debug_variant",
                "CoReWeBvIeW2_MaX_InStAnCeS",
            ]
            .map(OsString::from)
        );
        assert_eq!(plan.enforced_key, "COREWEBVIEW2_MAX_INSTANCES");
        assert_eq!(plan.enforced_value, "20");
    }
}
