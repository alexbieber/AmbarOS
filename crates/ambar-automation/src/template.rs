//! Minimal `{variable}` templating for workflow strings.
//!
//! Templates are deliberately tiny: no expressions, no filters, no shell.
//! Only a fixed set of variables is recognised, so a workflow cannot smuggle
//! arbitrary data into paths.

use std::collections::BTreeMap;

/// Variables available in every workflow.
pub const GLOBAL_VARS: &[&str] = &["date", "time", "datetime", "workflow_id", "execution_id"];

/// Variables only available when a file event triggered the workflow.
pub const FILE_VARS: &[&str] = &[
    "file",
    "file_name",
    "file_stem",
    "file_ext",
    "file_dir",
    "event",
];

/// Values substituted into templates at run time.
pub type Vars = BTreeMap<&'static str, String>;

/// Returns the variable names referenced by `template`, or an error message
/// describing a malformed placeholder.
pub fn referenced_vars(template: &str) -> Result<Vec<String>, String> {
    let mut vars = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(format!("unclosed `{{` in `{template}`"));
        };
        let name = &after[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return Err(format!("invalid placeholder `{{{name}}}` in `{template}`"));
        }
        vars.push(name.to_string());
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(format!("unmatched `}}` in `{template}`"));
    }
    Ok(vars)
}

/// Substitutes `{name}` placeholders. Unknown or unavailable variables are errors.
pub fn render(template: &str, vars: &Vars) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unclosed `{{` in `{template}`"))?;
        let name = &after[..end];
        let value = vars
            .get(name)
            .ok_or_else(|| format!("variable `{{{name}}}` is not available here"))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Expands a leading `~` to the user's home directory.
pub fn expand_home(path: &str) -> String {
    if (path == "~" || path.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        return format!("{}{}", home.to_string_lossy(), &path[1..]);
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_vars() {
        assert_eq!(
            referenced_vars("~/x/{date}/{file_name}").ok(),
            Some(vec!["date".to_string(), "file_name".to_string()])
        );
        assert!(referenced_vars("{oops").is_err());
        assert!(referenced_vars("{Bad}").is_err());
        assert!(referenced_vars("a}b").is_err());
    }

    #[test]
    fn renders() {
        let mut vars = Vars::new();
        vars.insert("file_name", "a.pdf".into());
        assert_eq!(
            render("got {file_name}!", &vars).ok(),
            Some("got a.pdf!".into())
        );
        assert!(render("{date}", &vars).is_err());
    }
}
