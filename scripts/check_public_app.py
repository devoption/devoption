#!/usr/bin/env python3
"""Read-only external HTTPS checks for the single-node acceptance runbook."""
import argparse
from datetime import datetime, timezone
import hashlib
import http.client
import ipaddress
import json
import socket
import ssl
import sys

from deployment_broker import hostname

MAX_BODY = 65536
TIMEOUT = 5


def resolve(host):
    return sorted({str(ipaddress.ip_address(item[4][0])) for item in
                   socket.getaddrinfo(host, 443, type=socket.SOCK_STREAM)})


def request(address, server_name, path, host=None):
    # Pin each connection to the address already checked against operator input.
    # No proxy environment, redirects, cookies, credentials or alternate CA input.
    context = ssl.create_default_context()
    with socket.create_connection((address, 443), timeout=TIMEOUT) as raw:
        with context.wrap_socket(raw, server_hostname=server_name) as stream:
            stream.sendall((f'GET {path} HTTP/1.1\r\nHost: {host or server_name}\r\n'
                            'Connection: close\r\nAccept-Encoding: identity\r\n\r\n').encode('ascii'))
            with http.client.HTTPResponse(stream) as response:
                response.begin()
                body = response.read(MAX_BODY + 1)
                if len(body) > MAX_BODY:
                    raise ValueError('response exceeds 64 KiB')
                return response.status, response.getheader('Location'), body


def check(platform, app, expected_addresses, health):
    hostname(platform)
    hostname(app)
    if platform == app:
        raise ValueError('platform and app hostnames must differ')
    parsed = {ipaddress.ip_address(value) for value in expected_addresses}
    if not parsed or len(parsed) > 8 or any(
            not a.is_global or a.is_multicast or getattr(a, 'scope_id', None) is not None for a in parsed):
        raise ValueError('supply one to eight public unicast node IP addresses')
    expected = {str(a) for a in parsed}
    health = health.encode('utf-8')
    if not health or len(health) > 1024:
        raise ValueError('expected health must contain 1 to 1024 UTF-8 bytes')
    report = {'checked_at': datetime.now(timezone.utc).isoformat(),
              'platform': platform, 'app': app, 'expected_addresses': sorted(expected),
              'expected_health_sha256': hashlib.sha256(health).hexdigest(),
              'checks': [], 'scope': 'External routing snapshot only; not full acceptance or ACME renewal proof.'}

    def record(name, passed, detail):
        report['checks'].append({'check': name, 'passed': passed, 'detail': detail})

    addresses = {}
    for host in (platform, app):
        try:
            found = resolve(host)
            passed = set(found) == expected
            record(f'DNS {host}', passed, {'resolved': found})
            if passed:
                addresses[host] = found
        except OSError:
            record(f'DNS {host}', False, 'Resolution failed; check DNS from this client.')

    def probe(name, address, server_name, path, status, body=None, location=None, host=None):
        try:
            actual, redirect, data = request(address, server_name, path, host)
            passed = actual == status and (body is None or data == body)
            if location is not None:
                passed = passed and redirect == location
            record(name, passed, f'HTTP {actual}; expected {status}' +
                   (' with exact health body' if body is not None else '') +
                   (' and sign-in redirect' if location else ''))
        except ssl.SSLCertVerificationError:
            record(name, False, 'Certificate verification failed; check hostname, expiry and public trust chain.')
        except ssl.SSLError:
            record(name, False, 'TLS failed; check proxy configuration and certificate issuance.')
        except (OSError, http.client.HTTPException, ValueError):
            record(name, False, 'Request failed or exceeded response limit; check listener, upstream and connectivity.')

    for address in addresses.get(platform, []):
        probe(f'Platform sign-in [{address}]', address, platform, '/sign-in', 200)
        probe(f'Unauthenticated projects [{address}]', address, platform, '/projects', 303, location='/sign-in')
        probe(f'Unknown HTTP Host [{address}]', address, platform, '/', 421, host='unbound.invalid')
    for address in addresses.get(app, []):
        probe(f'App health [{address}]', address, app, '/healthz', 200, body=health)
        for path in ('/auth', '/auth/session', '/sign-in', '/setup', '/setup/owner'):
            probe(f'App control path {path} [{address}]', address, app, path, 404)
        probe(f'App SNI/platform Host [{address}]', address, app, '/', 421, host=platform)
    report['passed'] = all(row['passed'] for row in report['checks'])
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform-hostname', required=True)
    parser.add_argument('--app-hostname', required=True)
    parser.add_argument('--expected-address', action='append', required=True,
                        help='public VM IP; repeat for every A/AAAA address')
    parser.add_argument('--expected-health', required=True, help='exact UTF-8 /healthz body, e.g. "ok A"')
    args = parser.parse_args(argv)
    try:
        report = check(args.platform_hostname, args.app_hostname, args.expected_address, args.expected_health)
    except ValueError as error:
        parser.error(str(error))
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
