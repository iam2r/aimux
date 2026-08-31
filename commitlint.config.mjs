export default {
	extends: ['@commitlint/config-conventional'],
	// apmux house style: conventional type(scope): subject, lowercase subject,
	// header ≤ 100 chars (defaults cover body-max-line-length=100).
	rules: {
		'header-max-length': [2, 'always', 100],
		'subject-case': [2, 'never', ['sentence-case', 'start-case', 'pascal-case', 'upper-case']],
	},
};
