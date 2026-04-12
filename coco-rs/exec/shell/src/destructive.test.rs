use super::*;

#[test]
fn test_detects_rm_rf() {
    assert!(get_destructive_warning("rm -rf /").is_some());
    assert!(get_destructive_warning("rm -rf ~").is_some());
    assert!(get_destructive_warning("rm -rf .").is_some());
    assert!(get_destructive_warning("rm -rf *").is_some());
    assert!(get_destructive_warning("rm file.txt").is_none());
}

#[test]
fn test_detects_force_push() {
    let warning = get_destructive_warning("git push --force origin main");
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("remote history"));

    assert!(get_destructive_warning("git push -f origin main").is_some());
}

#[test]
fn test_detects_git_destructive() {
    assert!(get_destructive_warning("git reset --hard HEAD~1").is_some());
    assert!(get_destructive_warning("git clean -fd").is_some());
    assert!(get_destructive_warning("git clean -f").is_some());
    assert!(get_destructive_warning("git checkout -- .").is_some());
    assert!(get_destructive_warning("git restore .").is_some());
    assert!(get_destructive_warning("git commit --no-verify -m msg").is_some());
}

#[test]
fn test_detects_sql_destructive() {
    assert!(get_destructive_warning("DROP TABLE users").is_some());
    assert!(get_destructive_warning("DROP DATABASE mydb").is_some());
    assert!(get_destructive_warning("TRUNCATE TABLE logs").is_some());
    // Case-insensitive SQL
    assert!(get_destructive_warning("drop table users").is_some());
    assert!(get_destructive_warning("Drop Database mydb").is_some());
    assert!(get_destructive_warning("delete from users where 1=1").is_some());
}

#[test]
fn test_detects_infrastructure_destructive() {
    assert!(get_destructive_warning("kubectl delete pod my-pod").is_some());
    assert!(get_destructive_warning("terraform destroy").is_some());
    assert!(get_destructive_warning("docker rm container_id").is_some());
    assert!(get_destructive_warning("docker rmi image_id").is_some());
    assert!(get_destructive_warning("docker system prune -a").is_some());
}

#[test]
fn test_detects_system_commands() {
    assert!(get_destructive_warning("shutdown -h now").is_some());
    assert!(get_destructive_warning("reboot").is_some());
    assert!(get_destructive_warning("kill -9 1234").is_some());
    assert!(get_destructive_warning("killall node").is_some());
}

#[test]
fn test_detects_disk_destructive() {
    assert!(get_destructive_warning("mkfs.ext4 /dev/sda1").is_some());
    assert!(get_destructive_warning("dd if=/dev/zero of=/dev/sda").is_some());
    assert!(get_destructive_warning("> /dev/sda").is_some());
}

#[test]
fn test_safe_commands_no_warning() {
    assert!(get_destructive_warning("ls -la").is_none());
    assert!(get_destructive_warning("git status").is_none());
    assert!(get_destructive_warning("cargo test").is_none());
    assert!(get_destructive_warning("cat file.txt").is_none());
    assert!(get_destructive_warning("git push origin main").is_none());
    assert!(get_destructive_warning("docker ps").is_none());
    assert!(get_destructive_warning("SELECT * FROM users").is_none());
}
