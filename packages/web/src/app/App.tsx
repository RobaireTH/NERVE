import { useEffect, useState } from 'react';
import { ContractorsPage } from '../features/employer/ContractorsPage';
import { LedgerPage } from '../features/employer/LedgerPage';
import { PortalPage } from '../features/portal/PortalPage';

function currentPath() {
	if (typeof window === 'undefined') return '/';
	return window.location.pathname || '/';
}

function navigate(next: string, setPathname: (value: string) => void) {
	if (typeof window === 'undefined') {
		setPathname(next);
		return;
	}
	window.history.pushState({}, '', next);
	setPathname(next);
}

export function App({ initialPath }: { initialPath?: string } = {}) {
	const [pathname, setPathname] = useState(initialPath ?? currentPath());

	useEffect(() => {
		if (typeof window === 'undefined') return;
		const onPopState = () => setPathname(currentPath());
		window.addEventListener('popstate', onPopState);
		return () => window.removeEventListener('popstate', onPopState);
	}, []);

	let content = <ContractorsPage />;
	if (pathname === '/ledger') {
		content = <LedgerPage />;
	} else if (pathname.startsWith('/portal/')) {
		content = <PortalPage token={pathname.replace('/portal/', '')} />;
	}

	return (
		<div className="shell">
			<aside className="rail">
				<div className="brand">Fiber Payroll</div>
				<nav className="nav-stack" aria-label="Primary">
					<a
						href="/"
						onClick={(event) => {
							event.preventDefault();
							navigate('/', setPathname);
						}}
					>
						Contractors
					</a>
					<a
						href="/ledger"
						onClick={(event) => {
							event.preventDefault();
							navigate('/ledger', setPathname);
						}}
					>
						Ledger
					</a>
				</nav>
			</aside>
			<main className="viewport">{content}</main>
		</div>
	);
}
