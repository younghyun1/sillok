//! Shell classification boundaries: quoting, substitutions, heredocs, wrappers,
//! Sillok writes, control-plane mutations, and scratch-space file writes.

use super::scan::{Scan, scan};
use super::tokenize::{Simple, split};
use std::path::Path;

const CWD: &str = "/work/repo";

fn change(script: &str) -> Option<String> {
    scan(script, Some(Path::new(CWD))).change
}

fn record(script: &str) -> bool {
    scan(script, Some(Path::new(CWD))).record
}

#[test]
fn splits_operators_quotes_and_redirections() {
    let commands =
        split(r#"FOO=1 git -C "a b" push 2>/dev/null && echo 'x; y' > out.txt | tee -a log"#);
    assert_eq!(
        commands,
        vec![
            Simple {
                words: vec![
                    "FOO=1".into(),
                    "git".into(),
                    "-C".into(),
                    "a b".into(),
                    "push".into()
                ],
                writes: vec!["/dev/null".into()],
            },
            Simple {
                words: vec!["echo".into(), "x; y".into()],
                writes: vec!["out.txt".into()]
            },
            Simple {
                words: vec!["tee".into(), "-a".into(), "log".into()],
                writes: vec![]
            },
        ]
    );
    assert_eq!(
        split("echo a >&2; echo b &>> both"),
        vec![
            Simple {
                words: vec!["echo".into(), "a".into()],
                writes: vec![]
            },
            Simple {
                words: vec!["echo".into(), "b".into()],
                writes: vec!["both".into()]
            },
        ]
    );
}

#[test]
fn heredoc_bodies_and_comments_are_not_commands() {
    let script = "cat > /dev/null <<'EOF'\ngit push origin main\nEOF\necho done # git push";
    assert_eq!(change(script), None);
    let tabbed = "cat <<-END\n\tgh pr merge 1\n\tEND\nls";
    assert_eq!(change(tabbed), None);
}

#[test]
fn quoted_text_is_data() {
    assert_eq!(change(r#"echo "git push" 'gh pr merge'"#), None);
    assert!(!record(r#"echo "sillok note x""#));
}

#[test]
fn detects_sillok_writes_in_every_usual_form() {
    for script in [
        r#"sillok note "Fixed it" --parent "$obj" --tags rust"#,
        r#"obj=$(sillok objective add "Ship it")"#,
        r#"x="$(sillok --at 2026-10-13T13:20 note "late")""#,
        "set obj (sillok objective add 'fish form')",
        "cd ~/soundpatrol; sillok objective complete 01a1 --note done",
        "sillok amend 01a1 --text fixed && sillok move 01a2 --parent 01a1",
        "bash -lc 'sillok retract 01a1 --reason dup'",
        "/Users/x/.cargo/bin/sillok --store /s note y",
        "echo `sillok note backtick`",
    ] {
        assert!(record(script), "{script}");
    }
    for script in [
        "sillok status",
        "sillok objective list",
        "sillok --human day",
        "sillok query --text x",
    ] {
        assert!(!record(script), "{script}");
    }
}

#[test]
fn detects_control_plane_mutations() {
    let cases = [
        ("git commit -m 'x' && git push", "git commit"),
        ("sudo -u peter git -C /work push", "git push"),
        ("gh pr merge 2155 --squash", "gh pr merge"),
        (
            "gh api -X PATCH repos/o/r/pulls/1 -f state=closed",
            "gh api",
        ),
        (
            "gh api repos/o/r/pulls/1/requested_reviewers -f 'reviewers[]=v'",
            "gh api",
        ),
        (
            "gh api graphql -f query='mutation { resolveReviewThread }'",
            "gh api",
        ),
        (
            "gcloud secrets versions add lynx --data-file=-",
            "gcloud secrets versions add",
        ),
        (
            "kubectl --context dev rollout restart deploy/x",
            "kubectl rollout restart",
        ),
        ("kubectl -n ns apply -f m.yaml", "kubectl apply"),
        ("tofu apply -auto-approve", "tofu apply"),
        ("gam update user will@x.com suspended on", "gam update"),
        (
            "op vault user revoke --vault Dev --user w",
            "op vault user revoke",
        ),
        (
            "aws identitystore delete-user --user-id 1",
            "aws identitystore delete-user",
        ),
        (
            "az billing profile update --name x",
            "az billing profile update",
        ),
        ("psql -c 'UPDATE songs SET x = 1'", "psql write"),
        ("curl -s -X POST https://api.example.com", "curl write"),
        ("timeout 30 gsutil -m rm gs://b/o", "gsutil rm"),
        ("rtk proxy git push", "git push"),
        ("env A=1 helm upgrade x ./chart", "helm upgrade"),
    ];
    for (script, expected) in cases {
        assert_eq!(change(script).as_deref(), Some(expected), "{script}");
    }
}

#[test]
fn reads_are_not_changes() {
    for script in [
        "git status && git log --oneline -5 && git diff",
        "gh pr view 2155 --json reviews",
        "gh api repos/o/r/pulls/1",
        "gh api -X GET repos/o/r/pulls -f state=open",
        "gh api graphql -f query='query { viewer { login } }'",
        "gcloud logging read 'severity>=ERROR' --limit 5 --format json",
        "gcloud compute instances list --project p",
        "kubectl get pods -n x -o wide",
        "kubectl rollout status deploy/x",
        "tofu plan",
        "gam print users",
        "op item get x --vault Dev",
        "aws s3 ls s3://bucket",
        "aws s3 cp s3://bucket/key ./local",
        "psql -c 'SELECT updated_at FROM songs'",
        "curl -s https://example.com",
        "gsutil iam get gs://b",
        "sed -n '1,20p' file.rs",
        "rg -n pattern src | head",
    ] {
        assert_eq!(change(script), None, "{script}");
    }
}

#[test]
fn file_writes_outside_scratch_count() {
    assert_eq!(
        change("echo x > notes.md").as_deref(),
        Some("write notes.md")
    );
    assert_eq!(
        change("sed -i '' 's/a/b/' src/main.rs").as_deref(),
        Some("sed main.rs")
    );
    assert_eq!(
        change("sed -i.bak -e 's/a/b/' -e 's/c/d/' a.txt").as_deref(),
        Some("sed a.txt")
    );
    assert_eq!(
        change("cp /tmp/build/out docs/out.pdf").as_deref(),
        Some("cp out.pdf")
    );
    assert_eq!(change("mv old.md new.md").as_deref(), Some("mv new.md"));
    assert_eq!(
        change("sd foo bar src/lib.rs").as_deref(),
        Some("sd lib.rs")
    );
    assert_eq!(change("echo x | sd foo bar"), None);
    if cfg!(unix) {
        assert_eq!(change("echo x >> /tmp/scratch.log"), None);
        assert_eq!(change("cp docs/a.md /tmp/a.md"), None);
        assert_eq!(change("cat a | tee /private/tmp/copy"), None);
    }
}

#[test]
fn follows_cd_and_variables_set_by_the_script() {
    assert_eq!(
        change("cd docs && echo x > a.md").as_deref(),
        Some("write a.md")
    );
    assert_eq!(change("echo x > $UNSET/a.md"), None);
    if cfg!(unix) {
        assert_eq!(
            change("W=/private/tmp/w; mkdir -p $W/out; cat > $W/BRIEF.md <<'EOF'\ngit push\nEOF"),
            None
        );
        assert_eq!(change("cd /tmp/x; sed -i '' 's/a/b/' 07-soc2.md"), None);
        assert_eq!(change("export D=/tmp/y; cp a.md \"${D}/b.md\""), None);
        assert_eq!(change("set W /tmp/z; echo > $W/q"), None);
        assert_eq!(
            change("cd /tmp/x; cd /work/repo; echo x > kept.md").as_deref(),
            Some("write kept.md")
        );
        assert_eq!(
            change("bash -c 'cd /tmp/q && rm f'; rm g").as_deref(),
            Some("rm g")
        );
    }
}

#[test]
fn unusual_input_never_panics() {
    for script in [
        "",
        "\\",
        "'",
        "\"",
        "$(",
        "`",
        "<<",
        "<<<",
        "2>",
        "&>",
        "echo $((1+2))",
        "a <(b) >(c)",
        "cat <<EOF",
        "((((((((((((sillok note deep))))))))))))",
        "\"$(\"",
        "x=`",
    ] {
        let _ = scan(script, None);
    }
    assert_eq!(scan("", None), Scan::default());
}
