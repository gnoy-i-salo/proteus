export default {
  extends: ['stylelint-config-standard-scss'],
  rules: {
    'selector-class-pattern':
      '^[a-z][a-z0-9]*(?:-[a-z0-9]+)*(?:__[a-z0-9]+(?:-[a-z0-9]+)*)?(?:--[a-z0-9]+(?:-[a-z0-9]+)*)?$',
    'custom-property-pattern': '^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$',
    'scss/dollar-variable-pattern': '^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$',
  },
};
