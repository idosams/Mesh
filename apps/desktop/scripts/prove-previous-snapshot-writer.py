import argparse, hashlib, json, pathlib, subprocess, time
p = argparse.ArgumentParser()
p.add_argument('--fixture', required=True)
p.add_argument('--executable', required=True)
p.add_argument('--output', required=True)
a = p.parse_args()
fixture_path, executable, output = map(pathlib.Path, (a.fixture, a.executable, a.output))
assert all(path.is_absolute() for path in (fixture_path, executable, output))
assert not output.exists(), 'preserve existing evidence'
fixture = json.loads(fixture_path.read_text())
assert fixture['schema'] == 'mesh.native-review-snapshot-fixture/v1'
expected_revision = 'dff7384212c0ed7422594fab06bc35dfcd092036'
expected_sha = '1af4edfe29e790f002d75840f8140f3d87df14ed19e7da9857f8ede96a974fd5'
def digest(path): return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
assert digest(executable) == expected_sha
identity = subprocess.run([str(executable), '--mesh-build-identity'], check=True, capture_output=True, text=True, timeout=30)
assert json.loads(identity.stdout) == {'schema': 'mesh.desktop-build-identity/v1', 'revision': expected_revision, 'exact': True}
paths = fixture['preserved_files']
assert paths and all(pathlib.Path(path).is_absolute() for path in paths)
before = {path: digest(path) for path in paths}
results = []
start = time.monotonic()
for registration in (fixture['owner_registration'], fixture['child_registration']):
    for action in ('versions', 'capture'):
        result = subprocess.run([str(executable), '--mesh-registered-attachment', action, fixture['storage'], registration], capture_output=True, text=True, timeout=30)
        assert result.returncode == 1, (action, result.returncode, result.stderr)
        assert result.stdout == '', 'refusal must not acknowledge saved history'
        assert {path: digest(path) for path in paths} == before, 'refused old writer changed retained evidence or project files'
        results.append({'registration': registration, 'action': action, 'exit_code': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
assert digest(executable) == expected_sha
proof = {'schema': 'mesh.previous-snapshot-writer-refusal/v1', 'revision': expected_revision, 'executable_sha256': expected_sha, 'fixture': str(fixture_path), 'fixture_sha256': digest(fixture_path), 'elapsed_seconds': time.monotonic() - start, 'preserved_sha256': before, 'commands': results, 'scope': 'Separate pre-snapshot executable processes refuse native owner and consumed-child versions/capture; no GUI or cached in-flight writer claim.'}
with output.open('x') as f: json.dump(proof, f, indent=2); f.write('\n')
print(json.dumps({'result': 'PASS', 'proof': str(output), 'elapsed_seconds': proof['elapsed_seconds']}))
