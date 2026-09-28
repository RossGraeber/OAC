// Self-test scenario: captures synthetic hazardous text and JSONL through ctx.capture, so
// the run's real redaction path (run.mjs finally block) is exercised end to end.
// --param kind=key|headers|frame|literal
const KEY = [
  'before the key',
  '-----BEGIN OPENSSH PRIVATE KEY-----',
  'b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW',
  'QyNTUxOQAAACDSyntheticBodyLineTwoXyZ0123456789abcdefABCDEF',
  'AAAEDSyntheticBodyLineThreeAbCdEf',
  '-----END OPENSSH PRIVATE KEY-----',
  'after the key',
].join('\n');

export default {
  name: 'selftest-capture',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: { kind: 'key' } },
  async run(ctx) {
    const kind = ctx.params.kind;
    if (kind === 'key') ctx.capture('key.txt', KEY);
    else if (kind === 'headers') {
      ctx.capture('headers.txt', '{"headers":{"Authorization":"Basic c2VjcmV0OnNlY3JldA=="}}\n{"Cookie":"session=abc123def456"}\nclean line');
      ctx.capture('headers.jsonl', '{"t":1,"direction":"http","headers":{"Authorization":"Basic c2VjcmV0OnNlY3JldA==","Cookie":"session=abc123def456"}}\n', { format: 'jsonl' });
    } else if (kind === 'frame') {
      ctx.capture('frame.jsonl', '{"jsonrpc":"2.0","method":"x","params":{"systemPrompt":"..."}}\n', { format: 'jsonl' });
    } else if (kind === 'literal') {
      const id = '0b5e8c1e-7a4f-4c2d-9e3b-5f1a2b3c4d5e';
      ctx.redactLiteral(id, '<INSTALLATION_ID>');
      ctx.capture('literal.jsonl', `{"installationId":"${id}","note":"ok"}\n`, { format: 'jsonl' });
      ctx.record('installationId', id);
    }
  },
};
