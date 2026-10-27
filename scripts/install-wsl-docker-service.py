#!/usr/bin/env python3
"""Explicit WSL host setup. Installs a boot hook; never restarts WSL or Docker."""
import configparser
import argparse
import datetime
import os
from pathlib import Path
import shutil
import tempfile


def install(source, system_root=Path('/'), replace_docker_command=False):
    conf = system_root / 'etc/wsl.conf'
    original = conf.read_text() if conf.exists() else ''
    parsed = configparser.ConfigParser(interpolation=None)
    parsed.read_string(original)
    if parsed.getboolean('boot', 'systemd', fallback=False):
        raise RuntimeError('WSL uses systemd; manage Docker with its native service instead')
    command = parsed.get('boot', 'command', fallback='')
    if command and 'dockerd' not in command and 'soapbox-docker-service' not in command:
        raise RuntimeError('An unrelated WSL boot command exists; integrate it explicitly first')
    if command and 'soapbox-docker-service' not in command and not replace_docker_command:
        raise RuntimeError('Review the existing Docker boot command, then use --replace-docker-boot-command')
    # Replace only the Docker boot command; preserve other sections/settings/comments.
    lines, output, in_boot, found_boot, replaced = original.splitlines(), [], False, False, False
    for line in lines:
        stripped = line.strip()
        if stripped.startswith('[') and stripped.endswith(']'):
            if in_boot and not replaced:
                output.append('command=/usr/local/sbin/soapbox-docker-service start')
                replaced = True
            in_boot = stripped.lower() == '[boot]'
            found_boot |= in_boot
        if in_boot and stripped.partition('=')[0].strip().lower() == 'command':
            output.append('command=/usr/local/sbin/soapbox-docker-service start')
            replaced = True
        else:
            output.append(line)
    if not found_boot:
        output.extend(['', '[boot]'])
    if not replaced:
        output.append('command=/usr/local/sbin/soapbox-docker-service start')
    updated = '\n'.join(output)+'\n'
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    backup = system_root / 'var/backups/soapbox-docker' / stamp
    backup.mkdir(parents=True, mode=0o700)
    (backup/'wsl.conf.before').write_text(original)
    for name, destination in [
        ('start-container-engine.sh', system_root/'usr/local/libexec/soapbox/start-container-engine.sh'),
        ('soapbox-docker-service', system_root/'usr/local/sbin/soapbox-docker-service'),
    ]:
        if destination.is_symlink():
            raise RuntimeError('Refusing to replace a symlink: '+str(destination))
        if destination.exists():
            shutil.copy2(destination, backup/name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as tmp:
            tmp.write((source/name).read_bytes())
        os.chmod(tmp.name, 0o755)
        os.replace(tmp.name, destination)
    if conf.is_symlink():
        raise RuntimeError('Refusing a symlinked WSL config')
    if conf.exists() and conf.read_text() != original:
        raise RuntimeError('WSL configuration changed during installation')
    with tempfile.NamedTemporaryFile(dir=conf.parent, delete=False) as tmp:
        tmp.write(updated.encode())
    os.chmod(tmp.name, 0o644)
    os.replace(tmp.name, conf)
    return backup


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--replace-docker-boot-command', action='store_true')
    args = parser.parse_args()
    if os.geteuid() != 0 or 'microsoft' not in Path('/proc/version').read_text().lower():
        raise SystemExit('Run explicitly as root in WSL without systemd')
    if Path('/proc/1/comm').read_text().strip() == 'systemd':
        raise SystemExit('Use the native systemd Docker service on this host')
    backup = install(Path(__file__).resolve().parent, replace_docker_command=args.replace_docker_boot_command)
    print(f'Installed persistent Docker boot service. Backup: {backup}')
    print('No services restarted. Pause jobs before using soapbox-docker-service restart.')
