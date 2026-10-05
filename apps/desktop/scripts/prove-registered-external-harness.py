import hashlib, json, pathlib, shutil, subprocess, threading, time

import argparse

parser = argparse.ArgumentParser(description='Controlled real external-Codex acceptance on a fresh retained synthetic native fixture.')
for option in ('fixture', 'executable', 'revision', 'sha256', 'output'):
    parser.add_argument('--' + option, required=True)
parser.add_argument('--provider', default=shutil.which('codex'))
options = parser.parse_args()
for value in (options.fixture, options.executable, options.output, options.provider):
    assert value and pathlib.Path(value).is_absolute(), 'Paths must be absolute'
assert len(options.revision) == 40 and all(c in '0123456789abcdef' for c in options.revision)
assert len(options.sha256) == 64 and all(c in '0123456789abcdef' for c in options.sha256)
manifest = pathlib.Path(options.fixture)
fixture = json.loads(manifest.read_text())
assert fixture['schema'] == 'mesh.registered-capture-fixture/v1'
project = pathlib.Path(fixture['project'])
assert 'mesh-desktop-consumed-review-' in str(project) and project.is_dir()
for field in ('root', 'storage', 'project', 'owner', 'journal'):
    assert pathlib.Path(fixture[field]).is_absolute()
assert len(fixture['registration']) == 64 and all(c in '0123456789abcdef' for c in fixture['registration'])
binary, revision, expected_hash = options.executable, options.revision, options.sha256
provider_executable = options.provider
digest = lambda p: hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
assert digest(binary) == expected_hash
output = pathlib.Path(options.output)
output.mkdir(exist_ok=False)
started = time.monotonic()
children = []
logs = []

def command(args, cwd=None):
    p = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=60)
    assert p.returncode == 0, (args, p.returncode, p.stderr)
    return p.stdout

def mesh(action):
    return [binary, '--mesh-registered-attachment', action, fixture['storage'], fixture['registration']]

def versions():
    return json.loads(command(mesh('versions')))['versions']

def preview(version):
    value = json.loads(command([*mesh('preview'), version, 'note.txt']))
    assert value['schema'] == 'mesh.attachment-text/v1'
    assert value['operation'] == version and value['path'] == 'note.txt'
    assert value['state'] == 'text'
    return value

def wait_until(predicate, description, processes, seconds=150):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate(): return
        for p in processes:
            assert p.poll() is None, (description, 'process exited', p.pid, p.returncode)
        time.sleep(.1)
    raise AssertionError('Deadline: ' + description)

def live(p):
    assert p.poll() is None, ('not alive', p.pid, p.returncode)

def launch(args, name, cwd=None, stdout_pipe=False):
    err = (output / (name + '.stderr')).open('w'); logs.append(err)
    out = subprocess.PIPE if stdout_pipe else (output / (name + '.stdout')).open('w')
    if not stdout_pipe: logs.append(out)
    p = subprocess.Popen(args, cwd=cwd, stdin=subprocess.PIPE, stdout=out, stderr=err, text=True)
    children.append(p)
    return p

try:
    identity = command([binary, '--mesh-build-identity'])
    assert json.loads(identity) == {'schema': 'mesh.desktop-build-identity/v1', 'exact': True, 'revision': revision}
    (output / 'build-identity.json').write_text(identity)
    before = versions()
    assert len(before) == 4, 'Use a fresh exported native fixture'
    assert not any((project / name).exists() for name in ('.mesh-proof-continue-1', '.mesh-proof-continue-2'))
    assert not (project / '.git').exists(), 'Never replace an existing Git repository'
    command(['git', 'init', '--initial-branch=checkpoint'], project)
    command(['git', 'add', 'note.txt'], project)
    command(['git', '-c', 'user.name=Mesh Acceptance', '-c', 'user.email=acceptance@example.invalid', 'commit', '-m', 'Synthetic external harness baseline'], project)
    head = command(['git', 'rev-parse', 'HEAD'], project)
    index = digest(project / '.git/index')
    inode = project.stat().st_ino
    owner = pathlib.Path(fixture['owner']) / 'note.txt'
    owner_before = owner.read_bytes()
    # Mesh commands must not modify Git even before a provider is launched.
    assert command(['git', 'rev-parse', 'HEAD'], project) == head
    assert digest(project / '.git/index') == index
    provider_options = ['exec', '--ignore-user-config', '--sandbox', 'workspace-write', '--json', '--ephemeral', '--color', 'never', '-c', 'approval_policy="never"', '-']
    provider = launch([provider_executable, *provider_options], 'provider', project)
    prompt = '''This is a controlled product acceptance test in a synthetic repository. Do not inspect other folders, change Git, start agents, access network yourself, or edit any file other than note.txt. Run one Python 3 script that does the following in this current directory: write exactly "external registered stage one\\n" to note.txt; wait up to 150 seconds for .mesh-proof-continue-1 to exist (sleep 0.1 seconds per check); then write exactly "external registered stage two\\n" to note.txt; wait up to 150 seconds for .mesh-proof-continue-2 to exist; then write exactly "external registered stage three\\n" to note.txt and exit. The external test controller will create the two gates. Fail if a gate times out. After the script finishes, reply only "Synthetic edits complete". Do not create or remove the gates yourself.'''
    provider.stdin.write(prompt); provider.stdin.close()
    note = project / 'note.txt'
    wait_until(lambda: note.read_text() == 'external registered stage one\n', 'provider first edit', [provider])
    live(provider)
    first_edit_elapsed = round((time.monotonic() - started) * 1000)
    watcher = launch(mesh('watch'), 'mesh-watch', stdout_pipe=True)
    events = []; parsing_errors = []
    def reader():
        with (output / 'mesh-watch.stdout').open('w') as logfile:
            for line in watcher.stdout:
                logfile.write(line); logfile.flush()
                if len(line) > 262144 or len(events) > 1024:
                    parsing_errors.append('oversized output'); watcher.kill(); break
                try: events.append(json.loads(line))
                except Exception as error: parsing_errors.append(str(error))
    thread = threading.Thread(target=reader, daemon=True); thread.start()
    saved = lambda: [e['saved_version'] for e in events if e.get('last_outcome') == 'saved' and e.get('saved_version') not in before]
    wait_until(lambda: bool(saved()), 'first external save', [provider, watcher])
    first_save = saved()[-1]
    first_preview = preview(first_save)
    assert first_preview['text'] == 'external registered stage one\n'
    (project / '.mesh-proof-continue-1').write_text('continue\n')
    wait_until(lambda: note.read_text() == 'external registered stage two\n', 'provider second edit', [provider, watcher])
    second_saves = []
    inspected = set()
    def second_saved_text():
        for version in saved():
            if version == first_save or version in inspected:
                continue
            value = preview(version)
            inspected.add(version)
            if value['text'] == 'external registered stage two\n':
                second_saves.append((version, value))
                return True
        return False
    wait_until(second_saved_text, 'second external saved bytes', [provider, watcher])
    second_save, second_preview = second_saves[0]
    assert second_save != first_save
    assert preview(first_save) == first_preview
    live(provider)
    watcher.stdin.write('stop\n'); watcher.stdin.close()
    assert watcher.wait(timeout=60) == 0
    thread.join(timeout=5); assert not thread.is_alive()
    assert not parsing_errors, parsing_errors
    assert events[-1]['phase'] == 'stopped'
    live(provider)
    after_stop = versions()
    assert len(after_stop) >= len(before) + 2
    assert after_stop[:len(before)] == before
    journal_at_stop = pathlib.Path(fixture['journal']).read_bytes()
    (project / '.mesh-proof-continue-2').write_text('continue\n')
    wait_until(lambda: note.read_text() == 'external registered stage three\n', 'post-stop external edit', [provider])
    assert provider.wait(timeout=90) == 0
    assert versions() == after_stop
    assert preview(first_save) == first_preview
    assert preview(second_save) == second_preview
    assert pathlib.Path(fixture['journal']).read_bytes() == journal_at_stop
    assert owner.read_bytes() == owner_before
    assert command(['git', 'rev-parse', 'HEAD'], project) == head
    assert digest(project / '.git/index') == index
    assert project.stat().st_ino == inode
    assert digest(binary) == expected_hash
    proof = dict(schema='mesh.registered-external-harness-acceptance/v2', passed=True, revision=revision, executable_sha256=expected_hash, provider_version=command([provider_executable, '--version']).strip(), provider_options=provider_options, provider_started_by_mesh=False, same_provider_alive_before_watch_and_after_stop=True, git_index_and_head_preserved=True, root_inode_preserved=True, owner_content_preserved=True, post_stop_external_edit_preserved=True, history_unchanged_after_stop=True, exact_saved_previews_preserved=True, first_saved_version=first_save, second_saved_version=second_save, first_saved_content_digest=first_preview['digest'], second_saved_content_digest=second_preview['digest'], versions_before=len(before), versions_after=len(after_stop), first_edit_elapsed_ms=first_edit_elapsed, elapsed_ms=round((time.monotonic()-started)*1000), graphical=False, packaged=False, protected_main_approval=False, limitations=['Controlled noninteractive provider process in synthetic registered consumed lane', 'Registration and consumed input prepared by native fixture', 'No graphical review, integration, restoration, second provider or second host verified', 'No matched velocity baseline or cost measurement'])
    (output / 'proof.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)
finally:
    for child in children:
        if child.poll() is None:
            child.terminate()
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired: child.kill(); child.wait(timeout=5)
    for logfile in logs: logfile.close()
