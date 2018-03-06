const debug = require('debug')
const assert = require('assert')

const Kernel = artifacts.require('./Kernel.sol')


// Valid Contracts
const Valid =  {
    Adder: artifacts.require('test/valid/Adder.sol'),
    Multiply: artifacts.require('test/valid/Multiply.sol'),
    Divide: artifacts.require('test/valid/Divide.sol')
}

// Test utility functions
function isNullAddress(address) {
    return address === "0x0000000000000000000000000000000000000000";
}

const testDebug = debug('test:Factory')
const testAccount = 0;

contract('Kernel', function (accounts) {

    describe('.listProcedures()', function () {
        it('should return nothing if zero procedures', async function() {
            let kernel = await Kernel.new();

            let procedures = await kernel.listProcedures.call();
            assert.equal(procedures.length, 0);
        })
        it('should return existing procedure keys', async function() {
            let kernel = await Kernel.new();

            let [err, address] = await kernel.createProcedure.call('TestAdder', Valid.Adder.bytecode)
            let tx1 = await kernel.createProcedure('TestAdder', Valid.Adder.bytecode)

            let procedures = await kernel.listProcedures.call();
            assert.equal(procedures.length, 1);
        });
        it('should return a list of procedures which can be retrieved', async function() {
            const kernel = await Kernel.new();
            const speccedProcedures =
                [ ["TestAdder", Valid.Adder],
                  ["TestDivider", Valid.Divide],
                  ["TestMultiplier", Valid.Multiply]
                ];
            for (const proc of speccedProcedures) {
                await kernel.createProcedure(proc[0], proc[1].bytecode)
            }

            const proceduresRaw = await kernel.listProcedures.call();
            const procedures = proceduresRaw.map(web3.toAscii).map(s=>s.replace(/\0.*$/, ''));

            // Test that the number of procedures stored is the same as the
            // number of procedures created
            assert.equal(procedures.length, speccedProcedures.length);
            // Cycle through each of the listed procedures
            for (const i in procedures) {
                // Test that the order and indexing of procedures is the same
                assert.equal(speccedProcedures[i][0], procedures[i])
                // Retrieve the listed procedure adress
                const address = await kernel.getProcedure.call(procedures[i]);
                // Check the address is correct
                assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`);
                assert(!isNullAddress(address), `Procedure Address (${address}) is not null`);
                // Check that the deployed code is the same as that sent
                const code = web3.eth.getCode(address);
                assert.equal(speccedProcedures[i][1].deployedBytecode, code);
            }
        });
    })
    describe('.getProcedure()', function () {
        it('should return a non-zero address iff procedure exists', async function () {
            let kernel = await Kernel.new();

            // Create "TestAdder"
            // Find the address (ephemerally)
            let [err, creationAddress] = await kernel.createProcedure.call('TestAdder', Valid.Adder.bytecode);
            assert(web3.isAddress(creationAddress), `Procedure Creation Address (${creationAddress}) is a real address`);
            assert(!isNullAddress(creationAddress), `Procedure Creation Address (${creationAddress}) is not null`);

            // Carry out the creation
            let tx1 = await kernel.createProcedure('TestAdder', Valid.Adder.bytecode);

            // Get the procedure
            let address = await kernel.getProcedure.call("TestAdder");
            assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`);
            assert(!isNullAddress(address), `Procedure Address (${address}) is not null`);

            assert.equal(creationAddress, address);
        });
        it('should return a zero address iff procedure does not exist', async function() {
            let kernel = await Kernel.new();
            // No procedures exist yet (nor does "TestAdder")
            let address = await kernel.getProcedure.call('TestAdder');
            assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
            assert(isNullAddress(address), `Procedure Address (${address}) is null`)
        });
    })

    describe('.createProcedure()', function() {
        it('should create valid procedure', async function () {
            let kernel = await Kernel.new();

            let [err, address] = await kernel.createProcedure.call('TestAdder', Valid.Adder.bytecode)
            let tx1 = await kernel.createProcedure('TestAdder', Valid.Adder.bytecode)

            assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
            assert(!isNullAddress(address), 'Procedure Address is not null')

            let adder = Valid.Adder.at(address);
            assert.equal(await adder.add.call(1, 1), 2)

            // The returned code should be the same as the sent code
            const code = web3.eth.getCode(address);
            assert.equal(Valid.Adder.deployedBytecode, code);
        });
        it('should create valid procedure (max key length)', async function () {
            const kernel = await Kernel.new();

            const name = "start123456789012345678901234end";
            assert.equal(name.length,32);
            const [err, address] = await kernel.createProcedure.call(name, Valid.Adder.bytecode)
            const tx1 = await kernel.createProcedure(name, Valid.Adder.bytecode)

            assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
            assert(!isNullAddress(address), 'Procedure Address is not null')

            const adder = Valid.Adder.at(address);
            assert.equal(await adder.add.call(1, 1), 2)

            // The returned code should be the same as the sent code
            const code = web3.eth.getCode(address);
            assert.equal(Valid.Adder.deployedBytecode, code);

            // The address should be gettable (TODO)
            // The correct name should be in the procedures table
            const proceduresRaw = await kernel.listProcedures.call();
            const procedures = proceduresRaw.map(web3.toAscii).map(s=>s.replace(/\0.*$/, ''));
            assert(procedures.includes(name), "The correct name is in the procedures table");
        });

        // TODO: what is an invalid payload?
        it('should reject invalid payload')

        describe('should reject invalid key', function () {
            // TODO: this is currently handle by truffle which simply
            // truncates the the string.
            it.skip('excess length', async function() {
                const kernel = await Kernel.new();
                const name = "start1234567890123456789012345678901234567890end";
                assert(name.length > 32, `Name length exceeds limit (length: ${name.length})`);

                const [err, address] = await kernel.createProcedure.call(name, Valid.Adder.bytecode)
                const tx = await kernel.createProcedure(name, Valid.Adder.bytecode)

                const proceduresRaw = await kernel.listProcedures.call();
                const procedures = proceduresRaw.map(web3.toAscii).map(s=>s.replace(/\0.*$/, ''));

                assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
                assert(isNullAddress(address), `Procedure Address (${address}) is null`)
            });
            it('zero length', async function() {
                let kernel = await Kernel.new();

                let [err, creationAddress] = await kernel.createProcedure.call('', Valid.Adder.bytecode)
                assert.equal(err,1);
                assert(web3.isAddress(creationAddress), `Procedure Creation Address (${creationAddress}) is a real address`)
                assert(isNullAddress(creationAddress), `Procedure Creation Address (${creationAddress}) is null`)

                const proceduresRaw = await kernel.listProcedures.call();
                const procedures = proceduresRaw.map(web3.toAscii).map(s=>s.replace(/\0.*$/, ''));
                assert.equal(procedures.length, 0);
                assert(!procedures.includes(''))

                const address = await kernel.getProcedure.call('');
                assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
                assert(isNullAddress(address), 'Procedure Address is null')
            });
            it('duplicate procedure key', async function() {
                const kernel = await Kernel.new();
                const name = "TestAdder";

                // This is the first time the procedure is added
                const [err1, address1] = await kernel.createProcedure.call(name, Valid.Adder.bytecode)
                const tx1 = await kernel.createProcedure(name, Valid.Adder.bytecode)

                // This is the second time the procedure is added
                const [err2, address2] = await kernel.createProcedure.call(name, Valid.Multiply.bytecode)
                const tx2 = await kernel.createProcedure(name, Valid.Multiply.bytecode)
                assert.equal(err2, 3);

                const proceduresRaw = await kernel.listProcedures.call();
                const procedures = proceduresRaw.map(web3.toAscii).map(s=>s.replace(/\0.*$/, ''));
                assert.equal(procedures.length, 1);

                const address = await kernel.getProcedure.call(name);
                assert.equal(address, address1);
                assert.notEqual(address, address2);
                assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
                assert(!isNullAddress(address), 'Procedure Address is not null')

                // The returned code should be the same as the original code
                const code = web3.eth.getCode(address);
                assert.equal(Valid.Adder.deployedBytecode, code);
            });
        })
    })

    describe('.deleteProcedure()', function () {
        it('should return error if procedure key does not exist')
        it('should return deleted procedure address if procedure key is valid ')

        // On deletion, kernel should destroy contract instance
        it.skip('should destroy the procedures contract on deletion', async function () {
            let kernel = new Kernel.new();

            let [err, address] = await kernel.createProcedure.call("test", Adder.bytecode);
            let tx1 = await kernel.createProcedure('test', Adder.bytecode)

            let delete_address = await kernel.deleteProcedure.call('test');
            let tx2 = await kernel.deleteProcedure('test');

            assert(false, "How do we test if deleted address is destroyed?")
        })

        describe('should reject invalid key', function () {
            it('excess length')
            it('zero length')
        })
    })

    describe('.executeProcedure(bytes32 key, bytes payload)', function () {

        describe('should return a valid value for', function () {
            it.skip('Adder Procedure', async function () {
                const kernel = await Kernel.new();
                let [err, address] = await kernel.createProcedure.call("TestAdder", Adder.bytecode);
                let tx = await kernel.createProcedure("TestAdder", Adder.bytecode);
                assert(web3.isAddress(address), `The returned address (${address}) is a valid address`);
                assert(!isNullAddress(address), `The returned address (${address}) is not the null address`);

                let tl = await kernel.executeProcedure("TestAdder");
                let tc = await kernel.executeProcedure.call("TestAdder");
                assert.equal(tc.toNumber(), 2);
            })
        })

        it('should return an error if key does not exist')

        describe('should return an error if procedure return error when', function () {
            it('recieves invalid arguments')
            it('throws an error', async function() {
                let kernel = await Kernel.new();

                let [err, address] = await kernel.createProcedure.call('TestDivide', Valid.Divide.bytecode)
                let tx1 = await kernel.createProcedure('TestDivide', Valid.Divide.bytecode)

                assert(web3.isAddress(address), `Procedure Address (${address}) is a real address`)
                assert(!isNullAddress(address), 'Procedure Address is not null')

                let divide = Valid.Divide.at(address);
                assert.equal(await divide.divide.call(8, 2), 4);

                // The returned code should be the same as the sent code
                const code = web3.eth.getCode(address);
                assert.equal(Valid.Divide.deployedBytecode, code);

                // Try dividing by zero
                try {
                    const divideByZero = await divide.divide.call(8, 0);
                } catch (e) {
                    assert.equal(e,"Error: VM Exception while processing transaction: invalid opcode");
                }
            });
        })


    })
})