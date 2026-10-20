"""Negative control: normalization alone misses leading dot components."""
import posixpath


def validate_manifest(manifest):
    if not isinstance(manifest, dict) or not isinstance(manifest.get('files'), list):
        raise ValueError('files must be an array')
    seen = set()
    for path in manifest['files']:
        if (not isinstance(path, str) or not path or path.startswith('/')
                or posixpath.normpath(path) != path
                or '\\' in path or '\x00' in path):
            raise ValueError('invalid path')
        if path in seen:
            raise ValueError('duplicate path')
        seen.add(path)
    return list(manifest['files'])
