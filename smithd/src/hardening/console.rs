// TODO(step 3): drop once the rule wires these up
#![allow(dead_code)]

const ACCESS_FILE: &str = "/etc/security/access-nologin.conf";
/// pam_access rule: deny every user from every origin
const ACCESS_RULE: &str = "-:ALL:ALL\n";
const PAM_LOGIN: &str = "/etc/pam.d/login";
/// same line forge writes. only /bin/login reads it, sshd and sudo have their own stacks
const PAM_LINE: &str =
    "account required pam_access.so accessfile=/etc/security/access-nologin.conf";

/// true when an active (uncommented) line matches ours, ignoring whitespace.
/// a commented-out copy doesn't count, unlike forge's grep
fn has_pam_line(pam: &str) -> bool {
    let wanted: Vec<&str> = PAM_LINE.split_whitespace().collect();
    pam.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#') && line.split_whitespace().eq(wanted.iter().copied())
    })
}

/// `pam` with our line appended, or `None` if it's already there
fn with_pam_line(pam: &str) -> Option<String> {
    if has_pam_line(pam) {
        return None;
    }
    let mut out = String::with_capacity(pam.len() + PAM_LINE.len() + 2);
    out.push_str(pam);
    if !pam.is_empty() && !pam.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(PAM_LINE);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // trimmed version of the stock l4t /etc/pam.d/login
    const STOCK: &str = "\
auth       optional   pam_faildelay.so  delay=3000000
# account  required       pam_access.so
@include common-auth
@include common-account
session    required   pam_limits.so
";

    #[test]
    fn appends_to_stock() {
        let Some(out) = with_pam_line(STOCK) else {
            panic!("expected the line to be added");
        };
        assert!(out.starts_with(STOCK));
        assert!(out.ends_with(&format!("{PAM_LINE}\n")));
        assert!(has_pam_line(&out));
    }

    #[test]
    fn leaves_it_alone_when_present() {
        let pam = format!("{STOCK}{PAM_LINE}\n");
        assert_eq!(with_pam_line(&pam), None);
    }

    #[test]
    fn whitespace_differences_still_count() {
        let pam = format!(
            "{STOCK}  account\trequired   pam_access.so accessfile=/etc/security/access-nologin.conf  \n"
        );
        assert!(has_pam_line(&pam));
    }

    #[test]
    fn commented_out_does_not_count() {
        let pam = format!("{STOCK}# {PAM_LINE}\n");
        assert!(!has_pam_line(&pam));
        assert!(with_pam_line(&pam).is_some());
    }

    #[test]
    fn stock_pam_access_comment_does_not_count() {
        assert!(!has_pam_line(STOCK));
    }

    #[test]
    fn other_accessfile_does_not_count() {
        let pam =
            format!("{STOCK}account required pam_access.so accessfile=/etc/security/access.conf\n");
        assert!(!has_pam_line(&pam));
    }

    #[test]
    fn adds_missing_trailing_newline() {
        let Some(out) = with_pam_line("@include common-account") else {
            panic!("expected the line to be added");
        };
        assert_eq!(out, format!("@include common-account\n{PAM_LINE}\n"));
    }

    #[test]
    fn writes_into_empty_file() {
        assert_eq!(with_pam_line(""), Some(format!("{PAM_LINE}\n")));
    }
}
