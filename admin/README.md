# tvserver sudo access

Install these files as `chris` from any directory:

```sh
sudo install -o root -g root -m 0755 /opt/tvserver/src-tauri/admin/tvserver-movie-perms /usr/local/sbin/tvserver-movie-perms
sudo visudo -cf /opt/tvserver/src-tauri/admin/tvserver-sudoers
sudo install -o root -g root -m 0440 /opt/tvserver/src-tauri/admin/tvserver-sudoers /etc/sudoers.d/tvserver-codex
sudo visudo -c
```

The sudoers rule permits passwordless `systemctl restart tvserver.service` and the
installed permissions helper. The helper accepts these commands:

```sh
sudo -n /usr/local/sbin/tvserver-movie-perms chmod 0755 '/tank/movies/The House of Eliott'
sudo -n /usr/local/sbin/tvserver-movie-perms acl tv rwx '/tank/movies/The House of Eliott'
sudo -n /usr/local/sbin/tvserver-movie-perms default-acl tv rwx '/tank/movies/The House of Eliott'
sudo -n /usr/bin/systemctl restart tvserver.service
```

`chmod` accepts modes 0640, 0644, 0660, 0664, 0700, 0750, 0755, 0770,
and 0775. ACL entries can be set for `chris` or `tv` using three `rwx`-style
characters. `default-acl` applies only to directories and governs newly created
children. Each invocation changes one existing file or directory. The helper
rejects paths outside `/tank/movies`, symlinks, mount crossings, and regular
files with multiple hard links.

The installed copy must remain owned by root and unwritable by `chris`; the
sudoers rule intentionally names that installed copy, not the repository file.
