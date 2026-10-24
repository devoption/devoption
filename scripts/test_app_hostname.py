import json
from pathlib import Path
import ssl
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import deployment_broker as broker
from configure_app_hostname import generate, render


class HostnameTests(unittest.TestCase):
    def setUp(self):
        self.binding = dict(platform_hostname='platform.example.com', app_hostname='app.example.com', project='a'*32, platform_port=3000)
        self.record = dict(project='a'*32, intent='running', health='ok A', invocation='b'*32)
        self.config = {'ingress': self.binding}

    def test_invalid_conflicting_and_injected_bindings_are_rejected(self):
        for host in ['platform.example.com', '*.example.com', 'APP.example.com', '127.0.0.1', 'app.example.com:443', 'app.example.com\nrespond 200', 'app.example.com/path', 'app..com', 'app.123']:
            with self.subTest(host=host), self.assertRaises(ValueError):
                render(self.binding | {'app_hostname': host}, 15000)
        for fields in [{'project': '../other'}, {'platform_port':15000}, {'platform_port':True}, {'extra':'directive'}]:
            with self.assertRaises(ValueError): render(self.binding | fields, 15000)

    def test_generated_pair_is_explicit_preserves_storage_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); source=root/'source.toml'
            fields=dict(uid=1000,build_work='/builds',state='/state',worker='/worker',port=15000,owner_data='/owner',history='/history',repositories='/repositories')
            source.write_text(''.join(f'{k} = {json.dumps(v)}\n' for k,v in fields.items()))
            generate(source,root/'generated',self.binding)
            proxy=(root/'generated/Caddyfile').read_text()
            self.assertIn('app.example.com {', proxy)
            self.assertIn('reverse_proxy 127.0.0.1:15000', proxy)
            self.assertIn('header_up -Cookie',proxy)
            self.assertNotIn('tls_insecure_skip_verify',proxy)
            self.assertNotIn('tls internal',proxy)
            import tomllib
            output=tomllib.loads((root/'generated/deployment.toml').read_text())
            self.assertEqual(output.pop('ingress'),self.binding)
            self.assertEqual(output,fields)
            with self.assertRaises(FileExistsError): generate(source,root/'generated',self.binding)

    def test_wrong_project_cannot_start_on_bound_hostname(self):
        with patch.object(broker,'load_release',return_value={'project':'c'*32}), patch.object(broker,'launch') as launch:
            with self.assertRaises(ValueError): broker.start(self.config | {'fingerprint':'f'},'release','ok A','f')
            launch.assert_not_called()

    def test_hostname_regeneration_preserves_existing_operator_record_binding(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'source.toml'
            fields=dict(uid=1000,build_work='/builds',state='/state',worker='/worker',port=15000,owner_data='/owner',history='/history',repositories='/repositories')
            source.write_text(''.join(f'{k}={json.dumps(v)}\n' for k,v in fields.items())+'\n[records]\ngrant_file="/etc/devoption/records.json"\n')
            generate(source,root/'generated',self.binding)
            import tomllib
            self.assertEqual(tomllib.loads((root/'generated/deployment.toml').read_text())['records'],{'grant_file':'/etc/devoption/records.json'})

    def test_status_explains_stopped_certificate_and_connection_failures(self):
        self.assertIsNone(broker.ingress_status({},None))
        self.assertIn('not deployed',broker.ingress_status(self.config,None)['status'])
        with patch.object(broker.ssl,'create_default_context',side_effect=ssl.SSLCertVerificationError('invalid')):
            self.assertIn('Certificate verification failed',broker.ingress_status(self.config,self.record)['status'])
        with patch.object(broker.socket,'create_connection',side_effect=ConnectionRefusedError):
            self.assertIn('ingress unavailable',broker.ingress_status(self.config,self.record)['status'])

    def test_absent_or_invalid_saved_invocation_cannot_verify_https(self):
        for invocation in [None, '', 'invalid']:
            with self.subTest(invocation=invocation), patch.object(broker.socket, 'create_connection') as connect:
                self.assertIn('does not match', broker.ingress_status(self.config, self.record | {'invocation': invocation})['status'])
                connect.assert_not_called()

    def test_status_requires_selected_invocation_and_health_over_verified_tls(self):
        response=MagicMock(); response.status=200; response.read.return_value=b'ok A'; response.getheader.return_value='b'*32
        with patch.object(broker.ssl,'create_default_context') as context, patch.object(broker.socket,'create_connection') as connect, patch.object(broker.http.client,'HTTPResponse',return_value=response):
            self.assertIn('certificate verified',broker.ingress_status(self.config,self.record)['status'])
            connect.assert_called_with(('127.0.0.1',443),timeout=2)
            self.assertEqual(context.return_value.wrap_socket.call_args.kwargs['server_hostname'],'app.example.com')
            response.getheader.return_value='other'
            self.assertIn('does not match',broker.ingress_status(self.config,self.record)['status'])
            response.status=502
            self.assertIn('cannot reach',broker.ingress_status(self.config,self.record)['status'])

if __name__ == '__main__': unittest.main()
