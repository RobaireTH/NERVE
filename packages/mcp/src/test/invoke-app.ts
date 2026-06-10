import { EventEmitter } from 'node:events';
import httpMocks from 'node-mocks-http';
import type { Express } from 'express';

interface InvokeOptions {
	method: string;
	url: string;
	body?: unknown;
}

export async function invokeApp(app: Express, options: InvokeOptions) {
	const request = httpMocks.createRequest({
		method: options.method,
		url: options.url,
		body: options.body,
		headers: {
			'content-type': 'application/json',
		},
	});
	const response = httpMocks.createResponse({
		eventEmitter: EventEmitter,
	});

	await new Promise<void>((resolve, reject) => {
		response.on('end', resolve);
		response.on('error', reject);
		app.handle(request, response, (err?: unknown) => {
			if (err) {
				reject(err);
				return;
			}
			resolve();
		});
	});

	const raw = response._getData();
	let json: unknown = raw;
	if (typeof raw === 'string' && raw.length > 0) {
		try {
			json = JSON.parse(raw);
		} catch {
			json = raw;
		}
	}

	return {
		status: response.statusCode,
		body: json,
	};
}
