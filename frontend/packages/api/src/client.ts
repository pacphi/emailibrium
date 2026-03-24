import ky from 'ky';

function addAuthHeader(request: Request): void {
  const token = localStorage.getItem('auth_token');
  if (token) {
    request.headers.set('Authorization', `Bearer ${token}`);
  }
}

export const api = ky.create({
  prefixUrl: '/api/v1',
  timeout: 30_000,
  hooks: {
    beforeRequest: [addAuthHeader],
  },
});
