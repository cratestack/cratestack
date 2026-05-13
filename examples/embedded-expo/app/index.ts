import { registerRootComponent } from 'expo';
import App from './App';

// Expo's registerRootComponent wires AppRegistry to React Native correctly
// regardless of the launch context (Expo Go, dev client, standalone build).
registerRootComponent(App);
